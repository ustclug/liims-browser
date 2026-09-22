#!/usr/bin/env python3
"""Package a binary built on Debian 13. Never installs into the host system."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

root = Path(__file__).resolve().parents[1]
version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
architecture = subprocess.check_output(["dpkg", "--print-architecture"], text=True).strip()
output = root / "dist"
output.mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix="liims-deb-") as temporary:
    stage = Path(temporary)
    subprocess.run(["make", "install", f"DESTDIR={stage}"], cwd=root, check=True)
    control = stage / "DEBIAN"
    control.mkdir()
    (control / "control").write_text(f"""Package: liims-browser
Version: {version}-1
Architecture: {architecture}
Maintainer: LIIMS maintainers <lug@ustc.edu.cn>
Depends: libgtk-4-1 (>= 4.18), libadwaita-1-0 (>= 1.7), libwebkitgtk-6.0-4 (>= 2.48)
Section: web
Priority: optional
Description: LIIMS public inquiry browser
 Native GTK4 and libadwaita browser with ephemeral reading sessions.
""")
    (control / "conffiles").write_text("/etc/liims/browser.toml\n")
    subprocess.run(["dpkg-deb", "--build", "--root-owner-group", str(stage),
                    str(output / f"liims-browser_{version}-1_{architecture}.deb")], check=True)
shutil.copy2(root / "target/release/liims-browser", output / "liims-browser")
