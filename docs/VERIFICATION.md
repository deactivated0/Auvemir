# Verification

Use [TRANSFORMATION.md](TRANSFORMATION.md) for this transformation's checks and limitations, [validation/](validation/) for machine-readable current evidence and [CAPTURES.md](CAPTURES.md) for genuine screenshot reproduction. Historical private validation identifiers are not part of the public verification record.

Run the locked Rust 1.99 release checks, real encoded codec fixtures and native Linux journeys from the README. Unit/integration tests cover persistence, schema rejection, malicious URLs, authenticated redirect boundaries, queue edits, feed parsing, download integrity, encrypted sync, casting relays, audiobook chapters, server paging, recording recovery, DSP and transport lifecycles. The new regressions cover atomic/resilient imports, stable sorting, stale result isolation, cancellation and persistent preferences.

`tests/verify_desktop.py` uses a disposable authenticated X display and profile. It checks actual import, native controls, queue/favorite state, decoder/output activity through a null ALSA sink, keyboard navigation, clean quit, restart persistence, viewport/DPI rendering and capture errors. A failed journey fails CI rather than being reported as a pass. Rendering a destination is recorded separately from completing an authenticated or physical-device journey.

`tests/verify_codecs.py` generates actual audio fixtures and checks decoded sample durations. `--extended` includes system FFmpeg fallback cases. These are format tests, not physical-output quality tests.

`tools/capture_screenshots.py` records actual Slint windows with generated demo audio and artwork. The phone views use the Linux responsive renderer. The navigation GIF records implemented animation; it is not a refresh-rate benchmark. Capture provenance includes source and image hashes.

Cross-platform CI, signing, real receivers, microphones, desktop portals, secure stores, screen readers, dynamic text and physical mobile devices must be checked in their own environments. No historical build result is silently carried forward as evidence for a changed source tree.
