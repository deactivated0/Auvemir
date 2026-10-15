# Auvemir

Private build validation for the Rust and Slint audio application. This is a development checkpoint; it does not complete the full requested product brief.

`Auvemir-Source.zip` contains the application, platform hosts, tests, packaging scripts, license notices and detailed implementation status. `ci-unpack.py` verifies its SHA-256 before each build and removes historical generated reports so artifacts describe the current job.

The manually dispatched validation workflow builds Linux x86_64/ARM64, Windows x86_64, macOS Intel/ARM64, Android ARM64/x86_64 and an unsigned iOS device package with simulator validation. Android native runtime tests target API 28 and 35. A separate artifact contains exact locked dependency sources and offline build configuration.

Builds remain private and artifacts expire after one day. No public release or deployment is performed by this workflow. Development APK signing keys generated on runners are test-only and are not included in source artifacts. Existing private signing material is not uploaded.

Previously validated source passed 67 Rust tests and Android headless playback, OS focus, screen replacement and microphone pause/resume/finalization checks. The current changes to recording notification actions, Android foreground-focus ordering and vector icons require fresh validation. Physical-device checks, signing/provisioning and the remaining product features are tracked in `docs/FEATURES.md` and `docs/PLATFORMS.md` inside the source archive.
