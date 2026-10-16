# Verification

The historical Linux checks below are retained for context. Current cross-platform evidence is in PLATFORMS.md. Private run 38877462432 built checkpoint 17 on Linux x86_64/ARM64, Windows MSVC, macOS Intel/ARM64 and Android ARM64/x86_64, and produced an unsigned iOS device package. Android API 28 and 35 passed the full native lifecycle, recording-notification and ambient-focus suite. iOS simulator compilation failed at an invalid bindgen target triple. Checkpoint 18 corrects that build flag and adds named presets, editable/tappable lyrics and mobile navigation polish. Its Linux local workspace passes 75 Rust tests and Clippy with warnings denied; fresh platform rebuilds remain required. Every CI artifact has its own validation-source.json, so historical build reports must not be treated as results for later source.

Checkpoint 22 adds on-demand M4B/M4A chapters, server pagination for Subsonic, Audiobookshelf, WebDAV and Invidious, nested WebDAV folder browsing, and Audiobookshelf progress read/resume/write. Tests cover Nero `chpl` lists, QuickTime tracks and precedence, empty metadata and invalid-file errors using an actual AAC/MP4 fixture; server mocks cover offsets, continuation, nested WebDAV requests, the second Audiobookshelf catalog page, and authenticated progress payloads. Desktop tests cover mapping a saved book time into its track and local offset. The checkpoint passed 85 workspace tests, Clippy with warnings denied and formatting. The phone Slint render showed the server paging control at 390×844; the Audiobooks page and expanded player were also rendered with chapter controls. The 25 upstream parser tests passed before its package was excluded from workspace linting. Private run 38891215707 validates checkpoint 22; its source identity job passed and platform jobs are in progress.

Checkpoint 23 fixes Invidious' 20-item provider pages to fill Auvemir's 100-item result windows without skipping results. A mocked search verifies offset 100 maps to provider pages 6–10 and returns 100 contiguous items. The full checkpoint 23 suite passes 85 tests, Clippy with warnings denied, and formatting. Private run 38891215707 is for checkpoint 22; checkpoint 23 still needs a current-source cross-platform run.

Linux x86_64 checks used Rust 1.99.0, Cargo.lock, Slint 1.18.1 and the actual release executable.

| Check | Result |
|---|---|
| Formatting and Clippy with warnings denied | Passed |
| Rust tests | 26 passed, 0 failed |
| Encoded audio fixtures | 14 passed, 0 failed |
| Slint rendering | Desktop home/library/settings and phone home/expanded player rendered; all five runs exited successfully |
| Package extraction | AppImage, DEB, RPM, Arch and portable archives contain the exact release executable; every extracted executable decoded the WAV fixture |
| Native output | CPAL/ALSA pipeline completed with a synthetic null sink; no physical speaker tested |
| MPRIS | Metadata title, volume change and remote quit passed using a private TCP test session bus |
| Live catalogs | Radio Browser: 100 BBC results; LibriVox: 23 Alice title-prefix results/RSS chapters; podcast directory: 50 Science results |
| Physical microphone, secure credential stores, native file portals | Not verified on real desktop hardware/session |
| Other platforms in this historical table | See PLATFORMS.md for newer native CI and Android runtime evidence |

The tests cover durable identity/settings/queues; SQLite migrations/future schema rejection; favorite preservation; literal search; bookmark and playlist cascades; scanner batching/unchanged skips; RSS/OPML/LRC; URL/DNS protections; download truncation/limits/resume entity validation; native noise bounds; EQ headroom; pitch-preserving 1.5× duration/pitch; persisted scrobble deduplication/backoff; history without the original file; protocol authentication separated from media identities; WebDAV filtering; all Audiobookshelf book tracks; restricted unofficial media rejection; and HTTP-success responses containing service errors.

## Codecs

`tests/verify_codecs.py` generates real one-second 440 Hz encoded fixtures and checks decoded sample duration. The final results are in `codec-results.json`.

Native checks: WAV, FLAC, MP3, Vorbis, M4A AAC, raw AAC, ALAC, AIFF, valid 128 kbit/s MP2 and mono Ogg Opus. Additional WMA v1/v2, WavPack and CAF checks include the optional system FFmpeg path where native support is absent. The CLI uses the application's actual decoder. This is not a broad corpus or a fidelity/device certification; chained/multichannel native Opus and many legacy variants remain unverified.

## Rendering and resource samples

`render-results.json` records five actual Slint captures; `performance-sample.json` records their short synthetic-output runs. The null ALSA sink is unpaced, so those CPU figures must not be interpreted as physical playback or idle CPU usage.

`idle-sample.json` records a separate empty-library capture with no audio output opened. It includes startup, a deliberately fixed two-second screenshot timer and shutdown. Neither sample measures first-frame latency, scrolling FPS, battery life or long-term playback. The release binary is approximately 36 MiB; sampled process RSS was approximately 36 MiB. Values change with artwork, libraries, sources and platform.

Encoded streaming storage is bounded at 1 MiB. PCM uses a bounded twelve-chunk ready queue, preallocated recycle buffers and 2048 frames per chunk. Capture has a separate bounded buffer pool/writer. Remote image requests are coalesced and limited to four slots, with bounded decoding and a 128 MiB disk cache. These limits do not substitute for real large-library and hardware benchmarks.

## Reproduce captures

Use a real display or Xvfb with the software backend:

```sh
SLINT_BACKEND=winit-software AUVEMIR_SCREENSHOT=/absolute/path/home.png AUVEMIR_SCREENSHOT_PAGE=0 target/release/auvemir
AUVEMIR_MOBILE=1 AUVEMIR_SCREENSHOT=/absolute/path/phone.png target/release/auvemir
```

`AUVEMIR_SCREENSHOT_PLAYER=1` opens the expanded player. The main UI closes after the two-second capture. Screenshot page flags only apply in screenshot mode. The included screenshots use a real synthetic WAV diagnostic track with Auvemir diagnostic metadata, not hardcoded commercial sample data. A new user's library is empty.

The test environment lacked normal Unix desktop sockets and X shared memory. A TCP X server and private anonymous test bus were used for rendering/MPRIS; this does not verify a real GNOME/KDE portal, native keyring prompt, Bluetooth device or physical audio route.

## Distribution limits

The Linux executable requires glibc 2.39+. Package extraction and decoding passed; actual installation/uninstallation and distribution coverage remain pending. Windows/macOS workflows, platform signing, mobile SDK/lifecycle work, live authenticated servers/scrobblers, physical capture/output, casting and all remaining roadmap features are explicitly separate from this verification.

## Current local regression pass

Checkpoint 18 passed 75 workspace Rust tests: 58 core, 15 desktop and two Android context tests. New cases cover legacy/named mix persistence across restart, preventing deleted legacy mixes from reappearing, rejecting invalid gains without replacing saved state, lyric-offset seek boundaries, stale lyric responses after manual edits, responses for another track, and retained manual lyrics/offset settings after reopening the database. Full workspace Clippy with warnings denied and actual Slint UI compilation passed. Native Android API 28/35 ambient tests passed at checkpoint 17; the new controls and iOS simulator correction require a new build. Flatpak packaging/runtime tooling has been added but has not yet executed.
