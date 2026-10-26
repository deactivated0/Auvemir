# Platform and package status

Current verification and remaining release work are recorded in [TRANSFORMATION.md](TRANSFORMATION.md). Historical internal run identifiers are intentionally omitted from this public document.

| Platform | Source and build support | Release validation still required |
| --- | --- | --- |
| Linux x86_64 | Native Rust/Slint release build, workspace checks, real codec fixtures and responsive UI verification; portable/DEB/RPM/Arch/AppImage packaging scripts | Physical audio and microphone, portal dialogs, secure-store/media-key integration, supported-distribution installation |
| Linux ARM64 | Shared native sources and release workflow matrix | Current-source native build and physical hardware checks |
| Windows x86_64 | MSVC build, static CRT release, portable ZIP and NSIS installer scripts | Current-source CI, interactive installation, WASAPI/SMTC/dialog/capture devices, signing |
| Windows ARM64 | Source may be portable; no supported package claimed | Toolchain/native dependencies and device validation |
| macOS Intel/ARM64 | Native sources, app ZIP/DMG scripts and CI matrix | Current-source CI, physical Core Audio/Keychain/microphone/media controls, signing/notarization |
| Android ARM64/x86_64 | Java host, native audio worker, foreground service, secure storage and APK signature/alignment checks | Current-source native build, lifecycle/touch/audio/storage/route checks on devices |
| iOS device/simulator | Swift host, Rust sources, Keychain, AirPlay/CarPlay integration and build scripts | Current-source Apple SDK build, provisioning, installation and physical AVAudioSession/Files/Keychain/AirPlay/CarPlay checks |
| Flatpak | Verified-binary packager and installed-runtime checks in source | Actual current-source builder/runtime execution; no Flathub publication claimed |

Linux packages built on Ubuntu 24.04 require glibc 2.39+. AppImage scripts bundle selected replaceable native libraries, excluding glibc, graphics drivers and session services. Flatpak targets the Freedesktop runtime and reuses a hash-verified native executable.

Desktop/mobile implementation and compilation do not establish physical-device readiness. Development APKs are not production identities. iOS installation requires an appropriate Apple signing identity and provisioning profile. No private keys, account secrets or profiles are included.
