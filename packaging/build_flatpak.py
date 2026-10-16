#!/usr/bin/env python3
"""Package a verified native Linux build and exercise its installed Flatpak runtime.

Install Flatpak, flatpak-builder, org.freedesktop.Platform//25.08 and the matching
SDK first. This creates a standalone development bundle, not a Flathub submission.
"""
import argparse
import hashlib
import json
import platform
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib
import wave

from source_identity import source_digest

ROOT = Path(__file__).resolve().parents[1]
APP_ID = "org.auvemir.Auvemir"
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]


def run(*command):
    return subprocess.run([str(item) for item in command], check=True,
                          capture_output=True, text=True).stdout


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/auvemir")
    parser.add_argument("--build-report", type=Path, default=ROOT / "docs/linux-build-results.json")
    args = parser.parse_args()
    if platform.machine() not in {"x86_64", "aarch64"}:
        raise RuntimeError("Flatpak packaging requires a native x86_64 or ARM64 Linux runner")
    for tool in ("flatpak", "flatpak-builder"):
        if not shutil.which(tool):
            raise RuntimeError(f"Install {tool} before packaging")
    native = json.loads(args.build_report.read_text())
    if digest(args.binary) != native["binary_sha256"] or source_digest() != native["source_sha256"]:
        raise RuntimeError("The native binary, source and validation report must describe the same build")
    report = {"source_sha256": native["source_sha256"], "binary_sha256": native["binary_sha256"],
              "runtime": "org.freedesktop.Platform//25.08", "installed_runtime_decode": False,
              "system_installation_tested": False, "physical_device_tested": False,
              "flathub_published": False}
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    destination = dist / f"Auvemir-{VERSION}-{platform.machine()}.flatpak"
    if destination.exists():
        raise FileExistsError("Choose a fresh output directory for Flatpak packaging")
    with tempfile.TemporaryDirectory(prefix="auvemir-flatpak-") as directory:
        work = Path(directory)
        payload = work / "payload"
        payload.mkdir()
        shutil.copy2(args.binary, payload / "auvemir")
        shutil.copy2(ROOT / "packaging/icons/auvemir.svg", payload / f"{APP_ID}.svg")
        desktop = (ROOT / "packaging/auvemir.desktop").read_text().replace("Icon=auvemir", f"Icon={APP_ID}")
        (payload / f"{APP_ID}.desktop").write_text(desktop)
        metadata = (ROOT / "packaging/org.auvemir.Auvemir.metainfo.xml").read_text().replace(
            ">auvemir.desktop<", f">{APP_ID}.desktop<")
        (payload / f"{APP_ID}.metainfo.xml").write_text(metadata)
        for name in ("LICENSE", "README.md", "THIRD_PARTY_NOTICES.md"):
            shutil.copy2(ROOT / name, payload / name)
        shutil.copytree(ROOT / "packaging/native-notices", payload / "native-notices")
        commands = ["install -Dm755 auvemir /app/bin/auvemir",
                    f"install -Dm644 {APP_ID}.desktop /app/share/applications/{APP_ID}.desktop",
                    f"install -Dm644 {APP_ID}.svg /app/share/icons/hicolor/scalable/apps/{APP_ID}.svg",
                    f"install -Dm644 {APP_ID}.metainfo.xml /app/share/metainfo/{APP_ID}.metainfo.xml",
                    "install -Dm644 LICENSE /app/share/licenses/auvemir/LICENSE",
                    "install -Dm644 README.md /app/share/doc/auvemir/README.md",
                    "install -Dm644 THIRD_PARTY_NOTICES.md /app/share/doc/auvemir/THIRD_PARTY_NOTICES.md",
                    "cp -r native-notices /app/share/doc/auvemir/"]
        manifest = {"app-id": APP_ID, "runtime": "org.freedesktop.Platform", "runtime-version": "25.08",
                    "sdk": "org.freedesktop.Sdk", "command": "auvemir", "branch": "stable",
                    "finish-args": ["--share=ipc", "--socket=fallback-x11", "--socket=wayland",
                                    "--socket=pulseaudio", "--device=dri", "--share=network",
                                    "--filesystem=xdg-music:ro", "--talk-name=org.freedesktop.secrets",
                                    "--own-name=org.mpris.MediaPlayer2.auvemir"],
                    "modules": [{"name": "auvemir", "buildsystem": "simple", "build-commands": commands,
                                 "sources": [{"type": "dir", "path": str(payload)}]}]}
        manifest_path = work / f"{APP_ID}.json"
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
        run("flatpak-builder", "--user", "--force-clean", "--disable-rofiles-fuse",
            f"--repo={work / 'repo'}", work / "build", manifest_path)
        bundle = work / destination.name
        run("flatpak", "build-bundle", work / "repo", bundle, APP_ID, "stable",
            "--runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo")
        run("flatpak", "install", "--user", "--noninteractive", "-y", bundle)
        report["user_installation_tested"] = True
        installed = run("flatpak", "run", "--command=sha256sum", APP_ID, "/app/bin/auvemir")
        if installed.split()[0] != native["binary_sha256"]:
            raise RuntimeError("Installed Flatpak executable differs from the verified native build")
        fixture = work / "runtime-check.wav"
        with wave.open(str(fixture), "wb") as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(16_000)
            wav.writeframes(b"\0\0" * 16_000)
        decoded = run("flatpak", "run", f"--filesystem={work}:ro", "--command=auvemir",
                      APP_ID, "--decode", fixture)
        if not re.search(r"Decoded 16000 samples.*1 channels.*16000 Hz", decoded):
            raise RuntimeError("The installed Flatpak did not decode the expected PCM fixture")
        report["installed_runtime_decode"] = True
        report["installed_binary_matches_release"] = True
        report["permissions"] = run("flatpak", "info", "--user", "--show-permissions", APP_ID)
        shutil.copy2(bundle, destination.with_suffix(".flatpak.part"))
        if digest(bundle) != digest(destination.with_suffix(".flatpak.part")):
            raise RuntimeError("The final Flatpak copy differs from the tested bundle")
        destination.with_suffix(".flatpak.part").replace(destination)
    report["artifact"] = destination.name
    report["sha256"] = digest(destination)
    (ROOT / "docs/flatpak-build-results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
