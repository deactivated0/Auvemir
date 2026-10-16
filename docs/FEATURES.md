# Implementation status

“Implemented” means connected source code, not validation on every platform or live account. This release does not complete the original product brief.

| Area | Implemented | Still required |
|---|---|---|
| Foundation | Rust core/desktop crates, Slint, SQLite migrations 1–5, bounded commands, lazy services | Stronger supervision/recovery and full lifecycle handling |
| Interface | Black desktop/phone layouts, home shortcuts, persistent/expanded player, real library views, forms and keyboard controls | Complete reference fidelity, artwork grids, full screen-reader/device interaction tests and virtualization |
| Library | Incremental scanning, tags, atomic tag editor, embedded/folder artwork, album/artist groups, favorites/search | Genre/composer/folder detail, exclusions, watcher, duplicate/stale-file management |
| Audio | CPAL, native decoding/Opus, bounded recycled PCM, producer-side EQ and pitch-preserving speed, seek/volume | Seamless queue gapless transitions, crossfade, ReplayGain, output picker, broad hardware profiling |
| Formats | 10 native fixtures plus four extended fixtures; optional FFmpeg | Broad real-world corpus, chained/multichannel Ogg Opus and device validation |
| Online music | Personal servers, authorized URLs, optional Invidious search/trending/direct AAC adapter | Native InnerTube extraction/accounts, full artist/album/playlists/recommendations, provider imports and sync |
| Servers | Subsonic/Navidrome songs, paged Subsonic/Audiobookshelf/WebDAV/Invidious results, nested WebDAV/Nextcloud folder browsing, Audiobookshelf catalog/book tracks/downloads and playback progress sync | Server bookmark sync, authenticated live validation |
| Books | Local folder grouping, LibriVox catalogs/RSS, chapter-file queues, embedded Nero/QuickTime chapters and seek, bookmarks, local resume, Audiobookshelf resume/progress sync, downloads/sleep | Book completion/narrator tags, smart rewind and seamless remote seeking |
| Podcasts | Directory search, RSS/Atom, subscriptions/manual refresh, episode progress/downloads, OPML | Scheduled refresh, auto-download/retention, chapters, silence skip and metered-network policy |
| Radio | Radio Browser failover/search, direct streams, custom stations/edit/delete, favorites and JSON/M3U import/export | ICY, remote playlist/HLS resolution, reconnect, browse filters and explicit stream test |
| Ambient | Generated noise/imported loops on shared mixer, per-layer volume, legacy and multiple named presets, OS media lifetime, mix pause/resume and focus restoration | Fresh named-preset UI/device checks, licensed nature loops, fade-out sleep timer and battery tests |
| Capture | Explicit capture, pause/resume, level meter, buffer pool, writer, normal-exit finalization, recovery of interrupted WAV files, native file export and Android recording notification actions | Physical device tests, trash/share, waveform and interruption/device recovery |
| Lyrics | Embedded lyrics, LRCLIB/LRC/cache, virtualized timed lines, offset persistence, import/export, local editor, coalesced bounded lookups and tap-to-seek | Fresh native UI interaction checks and broader LRC corpus |
| Queue/playlists | Add/remove/reorder/play-next, shuffle/repeat/clear, restored queue, saved playlist management/M3U | Entry editor, CUE and remote playlist sync |
| Downloads | Pause/resume/entity checks, limits, free-space checks, BLAKE3 manifest and identity preservation | Queue UI, checksum revalidation, quotas/retention and mobile scheduling |
| Artwork | Bounded image decoding, local thumbnails, coalesced requests, four network slots and 128 MiB disk cache | Exact visible-row fetching, cancellation and full LRU behavior |
| History/scrobbling | Actual listening-time history/statistics, JSON backup/import, persistent retry queue, multi-service ListenBrainz/Last.fm/Libre.fm/Maloja adapters and now-playing submissions | Live accounts, Last.fm authorization UI, detailed genre/album analytics |
| OS | MPRIS/SMTC/macOS transport/metadata adapters, native audio and dialogs | Full platform/device verification, notifications, audio focus/routes and mobile background lifecycle |
| Security | No telemetry, opt-in connections, native credentials, URL/DNS checks, response bounds, redirect isolation, SQL parameters | Native credential-store device checks, comprehensive threat/recovery review |
| Distribution | Private native Linux/Windows/macOS and Android builds, unsigned iOS device package; verified Linux packages; Flatpak build/runtime-check tooling | Current-source rebuilds, actual Flatpak execution, installation checks across distributions, iOS simulator correction, device checks and release signing |

## Additional requested integrations

| Status | Integrations |
|---|---|
| Implemented, authenticated live checks pending | Last.fm, ListenBrainz, Maloja, compatible Koito/Multi-Scrobbler endpoints; WebDAV/Nextcloud; Navidrome/Subsonic; Audiobookshelf |
| Implemented, platform checks separately recorded | MPRIS, Windows SMTC, macOS Now Playing; supported OS media keys can forward Bluetooth transport events |
| Implemented, device validation pending | Chromecast discovery/control and local file streaming; iOS AirPlay route picker; Android foreground media service and paged media browser; Discord local IPC; encrypted cross-device metadata sync |
| Implemented, API credentials/live checks pending | MusicBrainz, AcoustID fingerprints, Genius metadata/links, Cover Art Archive, Fanart.tv, Spotify/Deezer metadata imports with persistent unmatched tracks, Libre.fm and now-playing submissions |
| Implemented | LRCLIB; offline scrobble queue; multi-service/custom scrobbling; artist listening-time analytics; JSON history backup/import |
| Implemented; device/API checks pending | Encrypted Listen Together rooms; compatible YouTube Music public-playlist metadata imports with a user API key |
| Source added; Apple SDK/provisioning checks pending | CarPlay paged library browsing and transport through the existing engine |

No accounts, commercial audio, example libraries, or fake integration controls are bundled. Live Radio Browser, LibriVox and podcast discovery are checked separately from mocks of authenticated protocols. User-supplied credentials, real devices, platform SDKs and further independent implementation are still necessary for the full brief.
