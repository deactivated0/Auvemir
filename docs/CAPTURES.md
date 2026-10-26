# Reproducible screenshots

The README gallery is captured from the actual Slint window. `tools/create_demo_library.py` generates twelve 45-second quiet sine-tone FLAC files, explicit demo metadata, and procedural album covers. No commercial media or user profile is included. `tools/capture_screenshots.py` imports these files into a disposable profile and records the source digest, binary version and image hashes in `screenshots/manifest.json`.

On Linux install Python/Pillow, FFmpeg, Xvfb, xauth and xdotool, then run:

```sh
cargo build --locked --release -p auvemir -j 2
python3 tools/capture_screenshots.py target/release/auvemir
```

The script uses an authenticated disposable X display, local proxy endpoints that reject external catalog traffic and an ALSA null sink. Playback captures exercise real metadata/decoding/output code, but the unpaced null sink is not a physical output test. Phone captures exercise the responsive Linux layout, not Android/iOS devices. The navigation GIF encodes real native snapshot frames; it is not evidence of device refresh rate or animation FPS.

Developer capture controls are only enabled when `AUVEMIR_SCREENSHOT` is present:

| Variable | Behavior |
| --- | --- |
| `AUVEMIR_SCREENSHOT` | PNG output path; failures return nonzero after normal shutdown. |
| `AUVEMIR_SCREENSHOT_PAGE` | Initial navigation page, 0 home, 1 library, 11 settings. |
| `AUVEMIR_SCREENSHOT_SETTINGS` | Settings section, 0 playback, 1 appearance. |
| `AUVEMIR_SCREENSHOT_PLAYER` | Expand the real player. |
| `AUVEMIR_MOBILE` | Default capture size 390 × 844. |
| `AUVEMIR_SCREENSHOT_WIDTH/HEIGHT` | Logical dimensions, bounded to 320–2560 × 540–1800. |
| `AUVEMIR_SCREENSHOT_DELAY_MS` | 100–60000 ms, default 2000. |
| `AUVEMIR_CAPTURE_DIR` | Optional native PNG frame sequence directory. |
| `AUVEMIR_CAPTURE_FRAMES/FPS` | 2–240 frames, 1–30 frames/sec; defaults 100/20. |
| `AUVEMIR_REDUCE_MOTION=1` | Force reduced motion, useful for deterministic comparisons. |
| `SLINT_SCALE_FACTOR` | Native display scaling, useful for high-DPI verification. |

Normal application runs create no capture timer or frame sequence. Capture profiles and generated audio are temporary; only optimized PNG/GIF assets and the provenance manifest are committed.
