# Checkpoint 18

The original full product brief remains unfinished. This checkpoint implements named local ambient presets (with legacy migration, persistence, replacement/deletion and validation), a local lyrics editor, coalesced/bounded lookups with stale-response protection, timed-line tap-to-seek, persistent offset, virtualized lyrics, reference-style phone navigation, explicit iOS Clang deployment triples and Flatpak bundle/runtime validation tooling.

The local Linux workspace passed 75 Rust tests and full Clippy with warnings denied. Actual Slint software rendering checks cover phone home, named-preset controls, timed lyrics and the editor using clearly labeled test fixtures. These are layout checks, not mobile device interaction certification. New native platform builds, the iOS simulator correction and Flatpak execution still need validation.

Private run 38877462432 at source17 passed all five desktop targets' checks, release packaging and 14 codec fixtures; both Android architectures built and both API28/35 emulator suites passed native playback, focus, screen replacement, capture notification actions and ambient-only media controls. An unsigned iOS device package compiled. The simulator failed on the bindgen target spelling, corrected here. Exact source17 build reports remain separate from source18 local checks.

Packages remain development builds. Production signing/notarization, iOS provisioning and physical device checks are pending. No public release has been made. FEATURES.md tracks the remaining software work before final optimization and publication.
