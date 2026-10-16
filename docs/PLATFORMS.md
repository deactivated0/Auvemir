# Platform and package status

Evidence below is from private validation run 38877462432, source checkpoint 17 (`8ae8495e49d7a78a0e4f921b02f59bd8be86626e9b3c2fd2c71aa2e4b75e6bb4`). The later named-preset, lyrics, phone-navigation and iOS simulator build corrections in checkpoint 18 need fresh native builds. No public release has been made.

| Platform | Verified evidence at checkpoint 17 | Remaining |
|---|---|---|
| Linux x86_64 and ARM64 | Native Rust format/Clippy/tests, release, 14 codec fixtures, portable/DEB/RPM/Arch/AppImage binary and container checks | New checkpoint rebuild, actual installation, physical audio, portals, credentials and media keys |
| Windows x86_64 | Native MSVC checks, static CRT release, portable ZIP and NSIS installer, 14 actual decoder fixtures | New checkpoint rebuild, interactive installation, WASAPI/SMTC/dialog/capture hardware checks, signing |
| Windows ARM64 | Not built | Toolchain/native dependencies and device validation |
| macOS Intel and ARM64 | Native checks, release, 14 codec fixtures, app ZIP and DMG generation | New checkpoint rebuild, physical Core Audio/Keychain/microphone and media controls, signing/notarization |
| Android ARM64 | Actual native APK, Java/DEX compilation, signature and ELF/ZIP 16 KiB alignment | New checkpoint rebuild; physical ARM64 lifecycle, audio, storage, credentials and route checks |
| Android x86_64 | Actual native APK and signature/alignment; Android API 28 and 35 KVM runtime tests passed | Fresh new-UI interaction checks and physical devices |
| iOS ARM64 device | Rust/Swift native app and development ad-hoc signed IPA compiled; bundle, ARM64 executable and ZIP integrity checked | Provisioning and installable signing, physical AVAudioSession/Keychain/Files/AirPlay/CarPlay checks |
| iOS simulator | Build reached native SoundTouch bindings and failed because bindgen used an invalid simulator triple | Checkpoint 18 passes the explicit Clang deployment triple; fresh SDK build/launch/codecs still required |
| Flatpak | Checkpoint 18 adds a verified-binary packager and installed-runtime checks for both Linux architectures | Actual builder/runtime execution is pending; no Flathub publication |

Android API 28/35 exercised cold MediaBrowser startup and library import, native playback position, MediaSession pause/resume, transient OS focus, explicit user pause, Activity destruction/replacement, recording-notification pause/resume/stop with a new finalized WAV, and ambient-only OS media controls/focus recovery without restarting paused primary music. Neither run observed a System UI ANR. This is emulator evidence, not full touchscreen or physical-device certification. Re-signed deliverables retain the earlier development certificate and identical code/resources; runtime tests used the CI-signed original payload.

Linux packages require glibc 2.39+ because of the native build host. AppImage bundles selected replaceable native libraries, excluding glibc, drivers and session services. Flatpak packaging targets the Freedesktop 25.08 runtime and reuses a hash-verified native executable. It is a standalone bundle, not a Flathub submission.

Development APKs and desktop packages are not production-signed or notarized. The iOS device package needs a provisioning profile and an appropriate signing identity before installation. No credentials, Apple profiles or private development keys are included in source or public release assets.
