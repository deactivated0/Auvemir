# Auvemir

Private build validation for the native Rust and Slint audio application. This development checkpoint does not complete the full original product brief.

`Auvemir-Source.zip` contains the application, platform hosts, tests, packaging scripts, license notices and detailed status. `ci-unpack.py` verifies the reviewed archive SHA-256 and writes exact source/commit provenance before each job. Historical generated reports are removed so collected reports describe the current job.

Checkpoint 17 passed native checks and produced Linux x86_64/ARM64, Windows MSVC x86_64, macOS Intel/ARM64 and Android ARM64/x86_64 packages. All five desktop targets passed 14 decoder fixtures. Android API 28 and 35 KVM runtime tests passed cold browser/import, playback, OS focus, Activity replacement, recording-notification actions and ambient-only media controls. An unsigned iOS device package compiled; simulator compilation failed at an invalid bindgen target triple.

Checkpoint 18 adds named ambient presets with legacy migration, editable lyrics, bounded/coalesced lookups with stale-response protection, tap-to-seek timed lines, persistent lyric offset, reference-style mobile navigation, an explicit iOS Clang simulator triple, and Flatpak packaging/runtime tooling. Its local Linux workspace passed 75 tests and Clippy with warnings denied. These changes require fresh platform validation. Native dependency caches reduce repeated compilation on later runs.

The manually dispatched workflow builds all desktop and Android targets, an unsigned iOS device package and simulator, plus both Linux Flatpak bundles with installed-runtime checks. A separate artifact contains exact locked dependency source and offline configuration.

The repository and builds stay private. Artifacts expire after one day. This workflow creates no public release, deployment, production signing keys, or provisioning profiles. CI development APK keys are ephemeral and are not uploaded. Existing private signing material remains separate from source. Physical devices, production signing/notarization, authenticated accounts, remaining feature work and final optimization are tracked in `docs/FEATURES.md` and `docs/PLATFORMS.md` inside the source archive.
