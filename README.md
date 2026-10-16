# Auvemir

A native Rust + Slint player for music, personal music servers, free audiobooks, podcasts, radio, ambient mixes and voice recordings. The supplied black desktop and phone designs guide the interface, with persistent playback controls and an expanded player.

**This is a cross-platform development version. The entire supplied roadmap is not complete.** Shared desktop/mobile sources, encrypted metadata sync, casting and native platform hosts are implemented. Device validation and remaining product work are tracked in [FEATURES.md](docs/FEATURES.md) and [PLATFORMS.md](docs/PLATFORMS.md). Build reports distinguish compilation, signing checks and actual device testing.

## Run the Linux release

The binary requires **glibc 2.39 or newer**, ALSA, fontconfig, FreeType, libxkbcommon, libX11 and libstdc++. Install the DEB, RPM or Arch package matching your distribution, or extract the portable archive and run `./auvemir`. The AppImage includes selected replaceable audio/font/X11 libraries; it still needs glibc 2.39 and host graphics/session services. Compatibility with older distributions is not claimed.

File browsing uses an XDG desktop portal. Paths can also be entered directly. No microphone, network account or audio output is opened at startup. There is no telemetry.

## Features

- **Local library:** incremental file/folder scanning, atomic tag editing, metadata and artwork, albums, artists, search, favorites, playlists, restored queues, queue reordering, play-next insertion, shuffle and repeat.
- **Playback:** real CPAL output, bounded decoding outside the audio callback, local seeking, volume, ten-band EQ and pitch-preserving speed from 0.5× to 2×. SoundTouch and EQ run on the producer, with recycled PCM buffers.
- **Codecs:** native MP3, MP2, FLAC, WAV, Vorbis, AAC, ALAC, AIFF and mono/stereo Ogg Opus. Optional system FFmpeg supplies extended codecs after native decoding fails.
- **Servers:** Navidrome/Subsonic search and streaming; paged WebDAV/Nextcloud audio with nested folder browsing; paged Audiobookshelf libraries, individual book tracks, downloads and playback progress sync; Invidious search/trending. Server result pages contain up to 100 items; server bookmark sync remains pending.
- **Discovery:** LibriVox books and RSS chapters, Apple podcast directory search, RSS/Atom subscriptions, Radio Browser directory search/failover, local custom stations, and radio M3U/M3U8 or JSON import/export.
- **Optional unofficial catalog:** an explicitly configured Invidious instance supplies YouTube search/music trending and public direct AAC streams. Paid/premium/restricted items are rejected. No YouTube account, cipher solver, access-control bypass or online music downloads are included. Availability depends on the chosen instance and applicable service permissions.
- **Spoken audio:** per-track progress, resume, bookmarks, sleep timer, local/downloaded seeking, personal-server downloads and on-demand M4B/M4A chapter browsing for Nero chapter lists and QuickTime chapter tracks. Seamless remote whole-book seeking remains pending.
- **Downloads:** progress, pause, entity-validated HTTP Range resume, free-space checks, a 2 GiB file limit, partial files, atomic audio completion and a BLAKE3 manifest. The UI runs one download at a time.
- **Ambient:** shared-mixer noise and imported loops, individual layer volumes and one saved mix.
- **Recording:** explicit microphone start, pause/resume, level meter, WAV finalization, interrupted-recording recovery and native file export, using preallocated capture buffers and a separate writer. Physical microphone verification is pending.
- **Lyrics:** LRCLIB, local LRC, caching, current synchronized line, timing adjustment and import/export.
- **History/scrobbling:** listening-time analytics, JSON history backup/import, durable offline retries, ListenBrainz-compatible endpoints (including compatible Koito/Multi-Scrobbler configurations), Last.fm, Libre.fm and Maloja adapters, plus now-playing submissions. Music scrobbling is opt-in and counts listened time. Imported history is not automatically scrobbled. Live authenticated checks are pending.
- **Metadata/imports:** MusicBrainz, native AcoustID fingerprints, Genius metadata/links, Cover Art Archive, Fanart.tv and Spotify/Deezer/YouTube Music playlist metadata imports. Unmatched entries remain saved until audio is added. API credentials are supplied by the user.
- **Listen Together:** encrypted opt-in rooms synchronize matching, locally available seekable audio. Invitations include a private room key; audio files and account credentials are not shared. Local transport controls leave a followed room. Real-device timing checks remain pending.
- **Sync:** encrypted file transfer or WebDAV metadata sync for favorites, progress, playlists and selected playback settings, using Argon2id and XChaCha20-Poly1305. Audio files and credentials are excluded. Conflicts use deterministic revisions and deletion tombstones.
- **Casting:** Chromecast discovery/control, local range streaming and an authenticated-server relay that retains credentials on the sender. iOS includes an AirPlay route picker. A real receiver is required for device validation.
- **System integration:** MPRIS, Windows SMTC and macOS Now Playing adapters receive metadata and transport actions. Native validation is recorded separately from implementation.

## Connect a service

Settings → Servers and scrobbling accepts protocol, name, base URL, username and secret. Subsonic/WebDAV use a password; Audiobookshelf and ListenBrainz-compatible services use an API token. Last.fm expects JSON with `api_key`, `api_secret`, and an authenticated `session_key`. Maloja uses its API key. Invidious needs no secret and stays inactive until configured.

Credentials use Linux Secret Service, Windows Credential Manager or macOS Keychain. There is no plaintext fallback. Credential-backed connections cannot be saved if secure storage is unavailable. Base URLs must not contain query parameters, fragments or credentials. Authenticated requests never redirect to another host. TLS verification stays enabled. HTTP is permitted for an explicitly configured server; use HTTPS when transport confidentiality is required.

Android uses a process-lifetime application bridge and audio worker, replaceable UI attachments, KeyStore encryption and a native foreground service. Car browsing can initialize the worker without a phone activity. iOS uses Keychain and includes CarPlay browsing source; Apple compilation and approved provisioning remain pending. Settings also provides opt-in desktop Discord activity, encrypted sync and metadata API credentials.

## Build

Use Rust **1.99.0**, C/C++, CMake, Clang/libclang and native audio/font/window dependencies. Cargo.lock fixes dependency versions.

Debian/Ubuntu:

```sh
sudo apt install build-essential cmake clang libclang-dev pkg-config libasound2-dev libfontconfig1-dev libxkbcommon-dev libx11-dev
cargo build --locked --release -p auvemir -j 2
./target/release/auvemir
```

Arch/CachyOS:

```sh
sudo pacman -S --needed rust cmake clang pkgconf alsa-lib fontconfig freetype2 libxkbcommon libx11
cargo build --locked --release -p auvemir -j 2
```

Windows needs MSVC, CMake and LLVM; set `LIBCLANG_PATH` to LLVM's `bin` directory. macOS needs Xcode command-line tools, CMake and LLVM; set `LIBCLANG_PATH` to LLVM's `lib` directory. Use `-j 1` with limited build RAM. Release LTO is disabled after a reproduced Rust 1.99 thin-LTO linking failure; optimization and stripping remain enabled.

## Verify and package

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
python3 tests/verify_codecs.py target/release/auvemir --extended
python3 packaging/build_linux.py portable
python3 packaging/build_linux.py deb
python3 packaging/build_linux.py rpm
python3 packaging/build_linux.py arch
python3 packaging/build_appimage.py
```

Packaging needs `dpkg-deb`, `rpmbuild` and `zstd` for their formats. AppImage tooling is verified against SHA-256 from the upstream GitHub release. Windows NSIS and macOS app/DMG scripts are included. GitHub workflows build native desktop architectures, supply corresponding source and create a draft release after tag builds pass. Signing identities and platform verification are not fabricated.

On Windows, `python packaging/build_windows.py` creates a portable ZIP and NSIS installer; install NSIS first. MSVC packages use the static CRT. GNU cross builds require the matching runtime DLLs via `--runtime-dir`.

On Linux, `python3 packaging/bootstrap_android.py /path/to/android-sdk` installs verified SDK/NDK packages. Set `ANDROID_HOME`, `ANDROID_NDK_ROOT` and `JAVA_HOME` (JDK 17), add the Rust Android target, then run `python3 packaging/build_android.py --arch arm64`. The default APK uses a development key; a supplied `AUVEMIR_ANDROID_KEYSTORE`, alias and password enable your own signing identity. APK signature, ZIP alignment and native ELF alignment are checked. Preserve your signing key privately for updates.

`bash packaging/build_ios.sh` uses the Apple SDK on macOS, or xtool with a configured Apple SDK on Linux. The macOS route produces an ad-hoc signed IPA for subsequent provisioning. An Apple signing identity/profile is required for installation on a device. The mobile workflow builds Android packages and the unsigned iOS package without publishing them.

`auvemir --decode FILE` checks decoding. `auvemir --play FILE` checks output for at most 60 seconds. `AUVEMIR_DATA_DIR` overrides app data storage; `AUVEMIR_FFMPEG` selects the optional FFmpeg executable. Developer screenshot modes are documented in [VERIFICATION.md](docs/VERIFICATION.md).

## Source and license

`crates/core` owns SQLite, scanning, source identity, queues, feeds, catalogs, downloads, servers, lyrics and scrobbling. `crates/desktop` owns Slint, audio/DSP, capture, credentials, artwork and media controls. No Electron or embedded browser is used.

GPL-3.0-only. Slint uses its GPL option. Dependencies retain their licenses; SoundTouch uses LGPL-2.1. The corresponding-source archive includes the exact locked Cargo sources and build configuration. System FFmpeg is optional and is not bundled. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). No commercial audio, supplied reference artwork, credentials or user libraries are included.
