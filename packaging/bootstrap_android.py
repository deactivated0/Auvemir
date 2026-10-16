#!/usr/bin/env python3
"""Fetch pinned Android SDK packages from Google's repository and verify archive hashes."""
import argparse
import hashlib
from pathlib import Path
import shutil
import stat
import tempfile
import urllib.request
import xml.etree.ElementTree as ET
import zipfile

BASE = "https://dl.google.com/android/repository/"
PACKAGES = {"ndk;29.0.14206865":"ndk/android-ndk-r29", "platforms;android-36":"platforms/android-36", "build-tools;36.0.0":"build-tools/36.0.0", "platform-tools":"platform-tools"}

def child(node, name):
    return next((n for n in node if n.tag.split('}')[-1] == name), None)

def unpack(archive, destination):
    with zipfile.ZipFile(archive) as z:
        for info in z.infolist():
            output = (destination / info.filename).resolve()
            if not output.is_relative_to(destination.resolve()):
                raise ValueError("SDK archive contains an invalid path")
            mode = info.external_attr >> 16
            if info.is_dir():
                output.mkdir(parents=True, exist_ok=True)
                continue
            output.parent.mkdir(parents=True, exist_ok=True)
            if stat.S_ISLNK(mode):
                target = z.read(info).decode()
                if not (output.parent / target).resolve().is_relative_to(destination.resolve()):
                    raise ValueError("SDK archive contains an invalid link")
                output.symlink_to(target)
            else:
                with z.open(info) as source, output.open('wb') as target:
                    shutil.copyfileobj(source, target, 1024 * 1024)
                if mode & 0o777:
                    output.chmod(mode & 0o777)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=Path)
    args = parser.parse_args()
    root = args.destination.resolve()
    root.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(BASE + 'repository2-3.xml', timeout=60) as response:
        repository = ET.fromstring(response.read())
    for package, target in PACKAGES.items():
        destination = root / target
        if (destination / 'source.properties').is_file():
            print(f"Using {package}", flush=True)
            continue
        node = next(p for p in repository if p.tag.endswith('remotePackage') and p.attrib.get('path') == package)
        archives = child(node, 'archives')
        archive = next(a for a in archives if child(a, 'host-os') is None or child(a, 'host-os').text == 'linux')
        complete = child(archive, 'complete')
        url = child(complete, 'url').text
        checksum = child(complete, 'checksum')
        algorithm = checksum.attrib.get('type', 'sha1')
        expected = checksum.text.strip()
        print(f"Downloading {package}", flush=True)
        with tempfile.TemporaryDirectory(prefix='sdk-', dir=root) as temporary:
            work = Path(temporary)
            path = work / 'package.zip'
            digest = hashlib.new(algorithm)
            with urllib.request.urlopen(BASE + url, timeout=90) as source, path.open('wb') as output:
                while block := source.read(1024 * 1024):
                    digest.update(block)
                    output.write(block)
            if digest.hexdigest() != expected:
                raise RuntimeError(f"Checksum mismatch for {package}")
            contents = work / 'contents'
            unpack(path, contents)
            entries = list(contents.iterdir())
            source = entries[0] if len(entries) == 1 and entries[0].is_dir() else contents
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(source), str(destination))
        print(f"Verified and installed {package}", flush=True)
    print(f"ANDROID_HOME={root}", flush=True)
    print(f"ANDROID_NDK_ROOT={root / PACKAGES['ndk;29.0.14206865']}", flush=True)

if __name__ == '__main__':
    main()
