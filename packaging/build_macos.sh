#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
auvemir_version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml","rb"))["workspace"]["package"]["version"])')"
auvemir_arch="$(uname -m)"
mkdir -p dist/Auvemir.app/Contents/MacOS dist/Auvemir.app/Contents/Resources
cp target/release/auvemir dist/Auvemir.app/Contents/MacOS/
cp LICENSE README.md THIRD_PARTY_NOTICES.md dist/Auvemir.app/Contents/Resources/
python3 - "$auvemir_version" <<'PY'
from pathlib import Path
import plistlib, sys
data={"CFBundleExecutable":"auvemir","CFBundleIdentifier":"org.auvemir.Auvemir","CFBundleName":"Auvemir","CFBundleDisplayName":"Auvemir","CFBundlePackageType":"APPL","CFBundleShortVersionString":sys.argv[1],"CFBundleVersion":"1","LSMinimumSystemVersion":"13.0","NSMicrophoneUsageDescription":"Auvemir records voice only when you choose Record voice.","NSHighResolutionCapable":True,"CFBundleIconFile":"Auvemir"}
Path('dist/Auvemir.app/Contents/Info.plist').write_bytes(plistlib.dumps(data))
PY
auvemir_icons="$(mktemp -d)/Auvemir.iconset"
mkdir -p "$auvemir_icons"
for auvemir_size in 16 32 128 256 512; do
  sips -z "$auvemir_size" "$auvemir_size" packaging/icons/auvemir.png --out "$auvemir_icons/icon_${auvemir_size}x${auvemir_size}.png" >/dev/null
done
iconutil -c icns "$auvemir_icons" -o dist/Auvemir.app/Contents/Resources/Auvemir.icns
codesign --force --deep --sign - dist/Auvemir.app
hdiutil create -volname Auvemir -srcfolder dist/Auvemir.app -ov -format UDZO "dist/Auvemir-${auvemir_version}-macos-${auvemir_arch}.dmg"
ditto -c -k --sequesterRsrc --keepParent dist/Auvemir.app "dist/Auvemir-${auvemir_version}-macos-${auvemir_arch}.zip"
