# Auvemir

Private build validation for the native Rust and Slint audio application. This development checkpoint does not complete the full original product brief.

`Auvemir-Source.zip` contains the application, platform hosts, tests, packaging scripts, license notices and feature status. `ci-unpack.py` verifies the reviewed archive SHA-256 and writes exact source/commit provenance before each job. Historical generated reports are removed so collected reports describe the current job.

Checkpoint 22 adds embedded Nero and QuickTime M4B/M4A chapters, real provider paging for Subsonic, Audiobookshelf, WebDAV and Invidious, nested same-origin WebDAV/Nextcloud browsing, and Audiobookshelf playback-position resume and progress updates. Local validation passed 85 Rust tests, strict workspace Clippy, format checking, and focused mocked-server cases. This source checkpoint still needs a fresh cross-platform CI run.

The manually dispatched workflow validates Linux x86_64/ARM64, Windows x86_64, macOS Intel/ARM64, Android ARM64/x86_64 with API 28 and 35 runtime checks, iOS device/simulator packages, and Flatpak bundles. Artifacts are private and expire after one day. iOS builds use the macOS Xcode toolchain in this workflow; an xtool path is available in the local packaging script but has not been exercised in this run.

The repository and builds stay private. This workflow creates no public release, deployment, production signing keys or provisioning profiles. CI development APK keys are ephemeral and are not uploaded. Physical device casting, production signing/notarization, authenticated live integrations and remaining feature work are tracked in `docs/FEATURES.md` and `docs/PLATFORMS.md` inside the source archive.
