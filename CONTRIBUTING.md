# Contributing to Auvemir

Build with Rust 1.99.0 and the native dependencies in [BUILDING.md](docs/BUILDING.md). Fork the repository and target pull requests at `Main`. The upstream repository maintains a single branch; use your fork for topic branches.

Explain the user-visible problem and resulting behavior. Keep changes focused, preserve existing libraries and settings, and add regression coverage for meaningful behavior changes. Run formatting, release Clippy, release workspace tests and a release build before submission. Native UI changes should also run `tests/verify_desktop.py` and refresh genuine captures with `tools/capture_screenshots.py`.

Use generated fixtures or media you have permission to share. Never commit real profiles, tokens, room invitations, signed keys or private audio. Report the operating system, toolchain, reproduction steps and expected/actual behavior. Distinguish compilation from hardware, signing or authenticated-service validation.

Contributions use GPL-3.0-only and must preserve dependency notices. Release packaging must include corresponding source. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
