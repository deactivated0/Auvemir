#!/usr/bin/env python3
"""Assemble an AppImage using verified upstream tooling and replaceable shared libs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import tomllib
import urllib.request
from build_linux import populate, checksums, publish, ROOT, DIST

def download_asset(repo, name, destination):
    request = urllib.request.Request(f"https://api.github.com/repos/{repo}/releases/latest", headers={"User-Agent": "Auvemir-build"})
    release = json.load(urllib.request.urlopen(request, timeout=30))
    asset = next(a for a in release["assets"] if a["name"] == name)
    data = urllib.request.urlopen(asset["browser_download_url"], timeout=60).read()
    digest = "sha256:" + hashlib.sha256(data).hexdigest()
    if asset.get("digest") != digest:
        raise RuntimeError("Upstream artifact checksum is absent or does not match")
    destination.write_bytes(data)
    destination.chmod(0o755)
    return {"repository": repo, "tag": release["tag_name"], "asset": name, "sha256": digest[7:]}

def build(tool=None, runtime=None):
    arch = platform.machine()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    DIST.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="auvemir-appimage-") as directory:
        temporary = Path(directory)
        records = []
        if not tool:
            tool = temporary / "appimagetool.AppImage"
            records.append(download_asset("AppImage/appimagetool", f"appimagetool-{arch}.AppImage", tool))
            subprocess.run([str(tool), "--appimage-extract"], cwd=temporary, check=True, stdout=subprocess.DEVNULL)
            tool = temporary / "squashfs-root/AppRun"
        if not runtime:
            runtime = temporary / "runtime"
            records.append(download_asset("AppImage/type2-runtime", f"runtime-{arch}", runtime))
        app = temporary / "Auvemir.AppDir"
        populate(app)
        shutil.copy2(ROOT / "packaging/auvemir.desktop", app / "auvemir.desktop")
        shutil.copy2(ROOT / "packaging/icons/auvemir.svg", app / "auvemir.svg")
        (app / ".DirIcon").symlink_to("auvemir.svg")
        # Never bundle glibc, the loader, graphics drivers, or desktop session services.
        native = subprocess.check_output(["ldd", str(ROOT / "target/release/auvemir")], text=True)
        allowed = ("libasound", "libfontconfig", "libfreetype", "libxkbcommon", "libX11", "libXau", "libXdmcp", "libxcb", "libexpat", "libpng", "libbz2", "libz.so", "libbrotli", "libffi", "libuuid")
        libs = app / "usr/lib"
        libs.mkdir()
        for line in native.splitlines():
            if " => /" not in line:
                continue
            name, target = line.strip().split(" => ", 1)
            if name.startswith(allowed):
                shutil.copy2(target.split()[0], libs / name)
        (app / "AppRun").write_text('#!/bin/sh\nauvemir_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"\nexport LD_LIBRARY_PATH="$auvemir_dir/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"\nexec "$auvemir_dir/usr/bin/auvemir" "$@"\n')
        (app / "AppRun").chmod(0o755)
        package = temporary / f"Auvemir-{version}-{arch}.AppImage"
        subprocess.run([str(tool), "--no-appstream", "--runtime-file", str(runtime), str(app), str(package)], check=True, env={**os.environ, "ARCH": arch, "VERSION": version})
        publish(package)
        if records:
            (DIST / "appimage-tooling.json").write_text(json.dumps(records, indent=2) + "\n")
    checksums()

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--tool", type=Path)
    parser.add_argument("--runtime", type=Path)
    args = parser.parse_args()
    build(args.tool, args.runtime)
