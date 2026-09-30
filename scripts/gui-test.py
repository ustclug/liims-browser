#!/usr/bin/env python3
"""Run native Wayland tests on a private Weston display and isolated D-Bus bus."""

import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def run_inside():
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    processes = []
    try:
        read_fd, write_fd = os.pipe()
        with (runtime / "xvfb.log").open("w") as log:
            processes.append(
                subprocess.Popen(
                    [
                        "Xvfb",
                        "-displayfd",
                        str(write_fd),
                        "-screen",
                        "0",
                        "1920x1080x24",
                        "-nolisten",
                        "tcp",
                        "-ac",
                    ],
                    pass_fds=(write_fd,),
                    stdout=log,
                    stderr=log,
                )
            )
        os.close(write_fd)
        if not select.select([read_fd], [], [], 10)[0]:
            raise RuntimeError("Xvfb did not supply a display number")
        display = os.read(read_fd, 128).decode().strip()
        os.close(read_fd)
        if not display:
            raise RuntimeError("Xvfb failed: " + (runtime / "xvfb.log").read_text())
        os.environ["DISPLAY"] = ":" + display
        with (runtime / "weston.log").open("w") as log:
            processes.append(
                subprocess.Popen(
                    [
                        "weston",
                        "--backend=x11",
                        "--renderer=pixman",
                        "--socket=liims-test",
                        "--width=1600",
                        "--height=1000",
                        "--idle-time=0",
                    ],
                    stdout=log,
                    stderr=log,
                )
            )
        deadline = time.monotonic() + 10
        while not (runtime / "liims-test").exists():
            if processes[-1].poll() is not None or time.monotonic() > deadline:
                raise RuntimeError(
                    "Weston failed: " + (runtime / "weston.log").read_text()
                )
            time.sleep(0.05)
        os.environ.update(
            GDK_BACKEND="wayland",
            WAYLAND_DISPLAY="liims-test",
            GSK_RENDERER="cairo",
            GTK_A11Y="none",
            GIO_USE_VFS="local",
            LIBGL_ALWAYS_SOFTWARE="1",
            XDG_CURRENT_DESKTOP="weston",
            XDG_SESSION_TYPE="wayland",
        )
        return subprocess.run(
            [
                "cargo",
                "test",
                "--locked",
                "--offline",
                "--",
                "--ignored",
                "--test-threads=1",
                "--nocapture",
            ],
            cwd=ROOT,
            timeout=240,
        ).returncode
    finally:
        for process in reversed(processes):
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


if __name__ == "__main__":
    if "--inside" in sys.argv:
        sys.exit(run_inside())
    with tempfile.TemporaryDirectory(prefix="liims-gui-") as temporary:
        root = Path(temporary)
        env = os.environ.copy()
        for variable, directory in [
            ("XDG_RUNTIME_DIR", "run"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_DATA_HOME", "data"),
        ]:
            path = root / directory
            path.mkdir(mode=0o700)
            env[variable] = str(path)
        env["LIIMS_SCREENSHOT_DIR"] = str(ROOT / "target/screenshots")
        if env.get("GDK_SCALE", "1") != "1":
            env["LIIMS_SCREENSHOT_DIR"] += "/scale-" + env["GDK_SCALE"]
        if "--sites" in sys.argv:
            env["LIIMS_PROBE_SITES"] = str(ROOT / "target/site-smoke.tsv")
        sys.exit(
            subprocess.run(
                [
                    "bwrap",
                    "--bind",
                    "/",
                    "/",
                    "--dev-bind",
                    "/dev",
                    "/dev",
                    "--die-with-parent",
                    "--",
                    "dbus-run-session",
                    sys.executable,
                    str(Path(__file__).resolve()),
                    "--inside",
                ],
                env=env,
                cwd=ROOT,
            ).returncode
        )
