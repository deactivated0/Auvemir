# Building and packaging Auvemir

## Build

Use Rust **1.99.0**, C/C++, CMake, Clang/libclang and native audio/font/window dependencies. Cargo.lock fixes dependency versions.

Debian/Ubuntu:

```sh
sudo apt install build-essential cmake clang libclang-dev pkg-config libasound2-dev libfontconfig1-dev libxkbcommon-dev libxkbcommon-x11-0 libx11-dev
cargo build --locked --release -p auvemir -j 2
./target/release/auvemir
```

Arch/CachyOS:

```sh
sudo pacman -S --needed rust cmake clang pkgconf alsa-lib fontconfig freetype2 libxkbcommon libxkbcommon-x11 libx11
cargo build --locked --release -p auvemir -j 2
```

Windows needs MSVC, CMake and LLVM; set `LIBCLANG_PATH` to LLVM's `bin` directory. macOS needs Xcode command-line tools, CMake and LLVM; set `LIBCLANG_PATH` to LLVM's `lib` directory. Use `-j 1` with limited build RAM. Release LTO is disabled after a reproduced Rust 1.99 thin-LTO linking failure; optimization and stripping remain enabled.

## Verify and package

```sh
cargo fmt --all --check
cargo clippy --locked --release --workspace --all-targets -- -D warnings
cargo test --locked --release --workspace
python3 tests/verify_codecs.py target/release/auvemir --extended
python3 packaging/build_linux.py portable
python3 packaging/build_linux.py deb
python3 packaging/build_linux.py rpm
python3 packaging/build_linux.py arch
python3 packaging/build_appimage.py
python3 tests/verify_packages.py
```

Packaging needs `dpkg-deb`, `rpmbuild` and `zstd` for their formats. Package verification also needs `rpm2cpio`, `cpio` and the Linux capture dependencies from [CAPTURES.md](CAPTURES.md). It extracts all five formats, compares their executable with the release build, checks notices and exercises the AppImage's decoder and native window. AppImage tooling is verified against SHA-256 from the upstream GitHub release. Windows NSIS and macOS app/DMG scripts are included. GitHub workflows build native desktop architectures, supply corresponding source and create a draft release after tag builds pass. Signing identities and platform verification are not fabricated.

On Windows, `python packaging/build_windows.py` creates a portable ZIP and NSIS installer; install NSIS first. MSVC packages use the static CRT. GNU cross builds require the matching runtime DLLs via `--runtime-dir`.

On Linux, `python3 packaging/bootstrap_android.py /path/to/android-sdk` installs verified SDK/NDK packages. Set `ANDROID_HOME`, `ANDROID_NDK_ROOT` and `JAVA_HOME` (JDK 17), add the Rust Android target, then run `python3 packaging/build_android.py --arch arm64`. The default APK uses a development key; a supplied `AUVEMIR_ANDROID_KEYSTORE`, alias and password enable your own signing identity. APK signature, ZIP alignment and native ELF alignment are checked. Preserve your signing key privately for updates.

`bash packaging/build_ios.sh` uses the Apple SDK on macOS, or xtool with a configured Apple SDK on Linux. The macOS route produces an ad-hoc signed IPA for subsequent provisioning. An Apple signing identity/profile is required for installation on a device. The mobile workflow builds Android packages and the unsigned iOS package without publishing them.

`auvemir --decode FILE` checks decoding. `auvemir --play FILE` checks output for at most 60 seconds. `AUVEMIR_DATA_DIR` overrides app data storage; `AUVEMIR_FFMPEG` selects the optional FFmpeg executable. Developer screenshot modes are documented in [CAPTURES.md](CAPTURES.md).
