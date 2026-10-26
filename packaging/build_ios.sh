#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
auvemir_mode="${1:---device}"
if [[ "$auvemir_mode" != --device && "$auvemir_mode" != --simulator ]]; then
  echo 'Usage: build_ios.sh [--device|--simulator]' >&2
  exit 2
fi
if [[ "$(uname -s)" == Darwin && "${AUVEMIR_IOS_USE_XTOOL:-0}" == 1 && "$auvemir_mode" == --device ]]; then
  command -v xtool >/dev/null
  auvemir_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  export AUVEMIR_IOS_LIB_DIR="$auvemir_root/target/aarch64-apple-ios/release"
  export SDKROOT="${SDKROOT:-$(xcrun --sdk iphoneos --show-sdk-path)}"
  export IPHONEOS_DEPLOYMENT_TARGET=17.0
  export BINDGEN_EXTRA_CLANG_ARGS_aarch64_apple_ios="${BINDGEN_EXTRA_CLANG_ARGS_aarch64_apple_ios:-${BINDGEN_EXTRA_CLANG_ARGS:-}} --target=arm64-apple-ios17.0 -isysroot \"$SDKROOT\""
  rustup target add aarch64-apple-ios
  cargo rustc --locked --release -p auvemir --lib --crate-type staticlib --target aarch64-apple-ios -j 2
  (cd "$auvemir_root/platforms/ios" && xtool dev build --configuration release --ipa)
  test -f "$auvemir_root/platforms/ios/xtool/Auvemir.ipa"
  mkdir -p "$auvemir_root/dist"
  cp "$auvemir_root/platforms/ios/xtool/Auvemir.ipa" "$auvemir_root/dist/Auvemir-ios-unsigned.ipa"
  echo 'Built the iOS package with xtool. A provisioning profile and signing identity are required for device installation.'
  exit 0
fi
if [[ "$(uname -s)" == Darwin ]]; then
  auvemir_sdk=iphoneos
  auvemir_target=aarch64-apple-ios
  auvemir_swift_target=arm64-apple-ios17.0
  auvemir_platform=iPhoneOS
  auvemir_directory=ios
  if [[ "$auvemir_mode" == --simulator ]]; then
    auvemir_sdk=iphonesimulator
    auvemir_platform=iPhoneSimulator
    auvemir_directory=ios-simulator
    if [[ "$(uname -m)" == arm64 ]]; then
      auvemir_target=aarch64-apple-ios-sim
      auvemir_swift_target=arm64-apple-ios17.0-simulator
    else
      auvemir_target=x86_64-apple-ios
      auvemir_swift_target=x86_64-apple-ios17.0-simulator
    fi
  fi
  export SDKROOT="$(xcrun --sdk "$auvemir_sdk" --show-sdk-path)"
  export IPHONEOS_DEPLOYMENT_TARGET=17.0
  # bindgen 0.71 does not translate Rust's aarch64-apple-ios-sim spelling.
  # Give every native binding generator the actual Clang deployment triple.
  auvemir_bindgen_var="BINDGEN_EXTRA_CLANG_ARGS_${auvemir_target//-/_}"
  export "$auvemir_bindgen_var=${!auvemir_bindgen_var:-${BINDGEN_EXTRA_CLANG_ARGS:-}} --target=$auvemir_swift_target -isysroot \"$SDKROOT\""
  rustup target add "$auvemir_target"
  cargo rustc --locked --release -p auvemir --lib --crate-type staticlib --target "$auvemir_target" -j 2
  auvemir_app="$PWD/dist/$auvemir_directory/Payload/Auvemir.app"
  mkdir -p "$auvemir_app"
  cp platforms/ios/Info.plist "$auvemir_app/Info.plist"
  xcrun --sdk "$auvemir_sdk" swiftc -swift-version 5 -target "$auvemir_swift_target" -sdk "$SDKROOT" platforms/ios/Sources/Auvemir/*.swift -L "target/$auvemir_target/release" -lauvemir -lc++ -lsqlite3 -framework UIKit -framework AVFoundation -framework AVKit -framework MediaPlayer -framework CarPlay -framework AudioToolbox -framework CoreAudio -framework CoreFoundation -framework CoreGraphics -framework CoreText -framework Foundation -framework Security -framework Metal -framework MetalKit -framework CoreVideo -framework OpenGLES -framework QuartzCore -framework SystemConfiguration -framework UniformTypeIdentifiers -o "$auvemir_app/Auvemir"
  sips -z 120 120 packaging/icons/auvemir.png --out "$auvemir_app/AuvemirIcon@2x.png" >/dev/null
  sips -z 180 180 packaging/icons/auvemir.png --out "$auvemir_app/AuvemirIcon@3x.png" >/dev/null
  python3 - "$auvemir_app/Info.plist" "$auvemir_platform" <<'PY'
import plistlib,sys
from pathlib import Path
path=Path(sys.argv[1]);info=plistlib.loads(path.read_bytes())
info['CFBundleIcons']={'CFBundlePrimaryIcon':{'CFBundleIconFiles':['AuvemirIcon']}}
info['UIDeviceFamily']=[1,2]
info['CFBundleSupportedPlatforms']=[sys.argv[2]]
path.write_bytes(plistlib.dumps(info))
PY
  codesign --force --sign - "$auvemir_app"
  if [[ "$auvemir_mode" == --simulator ]]; then
    ditto -c -k --keepParent "$auvemir_app" dist/Auvemir-ios-simulator.zip
  else
    (cd dist/ios && ditto -c -k --keepParent Payload ../Auvemir-ios-unsigned.ipa)
    echo 'Built an unsigned iOS package. A provisioning profile and signing identity are required for installation.'
  fi
else
  if [[ "$auvemir_mode" == --simulator ]]; then echo 'Simulator packaging requires macOS and Xcode.' >&2; exit 2; fi
  : "${SDKROOT:?Set SDKROOT to an Apple SDK obtained through xtool setup or Xcode}"
  command -v xtool >/dev/null
  rustup target add aarch64-apple-ios
  cargo rustc --locked --release -p auvemir --lib --crate-type staticlib --target aarch64-apple-ios -j 2
  cd platforms/ios
  xtool dev build --configuration release --ipa
  test -f xtool/Auvemir.ipa
  mkdir -p ../../dist
  cp xtool/Auvemir.ipa ../../dist/Auvemir-ios-unsigned.ipa
fi
