#!/usr/bin/env python3
"""Build on Debian 13 with Rust 1.92+ and collect binary packages in dist/."""
from pathlib import Path
import shutil
import subprocess
import tomllib

root = Path(__file__).resolve().parents[1]
with open("/etc/os-release") as release:
    os_release = dict(line.rstrip().split("=", 1) for line in release if "=" in line)
if any(os_release.get(key, "").strip('"') != value
       for key, value in {"ID": "debian", "VERSION_ID": "13"}.items()):
    raise SystemExit("Build on Debian 13 or use packaging/Containerfile.debian")

version = subprocess.check_output(
    ["dpkg-parsechangelog", "-S", "Version"], cwd=root, text=True
).strip()
upstream = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
if version.rsplit("-", 1)[0] != upstream:
    raise SystemExit("Update debian/changelog to match Cargo.toml before packaging")
architecture = subprocess.check_output(["dpkg", "--print-architecture"], text=True).strip()
subprocess.run(["dpkg-buildpackage", "--build=binary", "--no-sign"], cwd=root, check=True)
output = root / "dist"
output.mkdir(exist_ok=True)
for package in ("liims-browser", "liims-browser-dbgsym"):
    filename = f"{package}_{version}_{architecture}.deb"
    shutil.copy2(root.parent / filename, output / filename)
for extension in ("buildinfo", "changes"):
    filename = f"liims-browser_{version}_{architecture}.{extension}"
    shutil.copy2(root.parent / filename, output / filename)
shutil.copy2(root / "debian/liims-browser/usr/bin/liims-browser", output / "liims-browser")
