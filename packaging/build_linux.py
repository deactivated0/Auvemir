#!/usr/bin/env python3
"""Build real Linux installation archives from an already verified release binary."""
import argparse
import errno
import hashlib
import platform
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import time
import tomllib

ROOT = Path(__file__).resolve().parent.parent
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
ARCH = platform.machine()
DEB_ARCH = {"x86_64":"amd64","aarch64":"arm64"}[ARCH]
DIST = ROOT / "dist"

def publish(source, name=None):
    """Expose only a closed, complete package to artifact collectors and users."""
    destination = DIST / (name or source.name)
    try:
        source.replace(destination)
    except OSError as error:
        if error.errno != errno.EXDEV:
            raise
        temporary = destination.with_name(destination.name + ".part")
        try:
            shutil.copy2(source, temporary)
            temporary.replace(destination)
        finally:
            temporary.unlink(missing_ok=True)

def populate(root):
    files = {
        ROOT / "target/release/auvemir": "usr/bin/auvemir",
        ROOT / "packaging/auvemir.desktop": "usr/share/applications/auvemir.desktop",
        ROOT / "packaging/icons/auvemir.svg": "usr/share/icons/hicolor/scalable/apps/auvemir.svg",
        ROOT / "LICENSE": "usr/share/doc/auvemir/LICENSE",
        ROOT / "README.md": "usr/share/doc/auvemir/README.md",
        ROOT / "THIRD_PARTY_NOTICES.md": "usr/share/doc/auvemir/THIRD_PARTY_NOTICES.md",
    }
    if (ROOT / "packaging/native-notices").is_dir():
        for notice in (ROOT / "packaging/native-notices").glob("*.txt"):
            files[notice]="usr/share/doc/auvemir/native-notices/"+notice.name
    for source, relative in files.items():
        destination = root / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)

def checksums():
    lines = []
    for path in sorted(DIST.iterdir()):
        if path.is_file() and path.name != "SHA256SUMS":
            with path.open("rb") as file:
                lines.append(f"{hashlib.file_digest(file, 'sha256').hexdigest()}  {path.name}\n")
    (DIST / "SHA256SUMS").write_text("".join(lines))

def build(kind):
    DIST.mkdir(exist_ok=True)
    binary = ROOT / "target/release/auvemir"
    if not binary.is_file():
        raise SystemExit("Build target/release/auvemir first")
    with tempfile.TemporaryDirectory(prefix="auvemir-package-") as directory:
        staging = Path(directory)
        root = staging / "root"
        populate(root)
        if kind == "deb":
            control = root / "DEBIAN"
            control.mkdir()
            (control / "control").write_text(f"""Package: auvemir
Version: {VERSION}
Section: sound
Priority: optional
Architecture: {DEB_ARCH}
Maintainer: Auvemir contributors <maintainers@auvemir.invalid>
Installed-Size: {sum(p.stat().st_size for p in root.rglob('*') if p.is_file()) // 1024}
Depends: libc6 (>= 2.39), libgcc-s1, libstdc++6, libasound2t64 | libasound2, libfontconfig1, libfreetype6, libxkbcommon0, libx11-6
Recommends: xdg-desktop-portal
Suggests: ffmpeg
Description: Auvemir music and spoken-audio player
 Native Rust and Slint player for music, books, podcasts, radio and ambient audio.
""")
            package = staging / f"auvemir_{VERSION}_{DEB_ARCH}.deb"
            subprocess.run(["dpkg-deb", "--root-owner-group", "--build", str(root), str(package)], check=True)
            publish(package)
        elif kind == "arch":
            (root / ".PKGINFO").write_text(f"""pkgname = auvemir
pkgbase = auvemir
pkgver = {VERSION}-1
pkgdesc = Native music and spoken-audio player
builddate = {int(time.time())}
packager = Auvemir contributors
size = {sum(p.stat().st_size for p in root.rglob('*') if p.is_file())}
arch = {ARCH}
license = GPL-3.0-only
depend = alsa-lib
depend = fontconfig
depend = freetype2
depend = libxkbcommon
depend = libx11
depend = gcc-libs
depend = glibc>=2.39
optdepend = ffmpeg: extended codecs
optdepend = xdg-desktop-portal: native file picker
""")
            tar = staging / "package.tar"
            with tarfile.open(tar, "w", format=tarfile.GNU_FORMAT) as archive:
                for path in sorted(root.rglob("*")):
                    if path.is_file():
                        info = archive.gettarinfo(str(path), str(path.relative_to(root)))
                        info.uid = info.gid = 0
                        info.uname = info.gname = "root"
                        with path.open("rb") as file:
                            archive.addfile(info, file)
            compressed=staging / "package.pkg.tar.zst"
            subprocess.run(["zstd", "-q", "-f", "-10", str(tar), "-o", str(compressed)], check=True)
            subprocess.run(["zstd", "-q", "-t", str(compressed)], check=True)
            publish(compressed, f"auvemir-{VERSION}-1-{ARCH}.pkg.tar.zst")
        elif kind == "portable":
            bundle = staging / f"auvemir-{VERSION}"
            shutil.copytree(root, bundle)
            (bundle / "auvemir").write_text('#!/bin/sh\nauvemir_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"\nexec "$auvemir_dir/usr/bin/auvemir" "$@"\n')
            (bundle / "auvemir").chmod(0o755)
            package = staging / f"auvemir-{VERSION}-linux-{ARCH}.tar.gz"
            with tarfile.open(package, "w:gz") as archive:
                archive.add(bundle, arcname=bundle.name)
            publish(package)
        elif kind == "rpm":
            top = staging / "rpm"
            for name in ["BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"]:
                (top / name).mkdir(parents=True)
            spec = top / "SPECS/auvemir.spec"
            spec.write_text(f"""Name: auvemir
Version: {VERSION}
Release: 1
Summary: Native music and spoken-audio player
License: GPL-3.0-only
BuildArch: {ARCH}
AutoReqProv: no
Requires: alsa-lib, fontconfig, freetype, libxkbcommon, libX11, libstdc++, glibc >= 2.39
%description
Auvemir plays music, books, podcasts, radio and ambient audio with Rust and Slint.
%install
mkdir -p %{{buildroot}}
cp -a {root}/usr %{{buildroot}}/
%files
/usr/bin/auvemir
/usr/share/applications/auvemir.desktop
/usr/share/icons/hicolor/scalable/apps/auvemir.svg
%license /usr/share/doc/auvemir/LICENSE
/usr/share/doc/auvemir/README.md
/usr/share/doc/auvemir/THIRD_PARTY_NOTICES.md
/usr/share/doc/auvemir/native-notices
""")
            subprocess.run(["rpmbuild", "--define", f"_topdir {top}", "--define", "__os_install_post %{nil}", "--define", "_build_id_links none", "-bb", str(spec)], check=True)
            for package in (top / "RPMS").rglob("*.rpm"):
                publish(package)
        else:
            raise SystemExit(f"Unknown package type: {kind}")
    checksums()

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=["portable", "deb", "arch", "rpm"])
    build(parser.parse_args().kind)
