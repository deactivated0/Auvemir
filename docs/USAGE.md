# Listening with Auvemir

Start with Library, then Add files or Add folder. A typed path is available on desktop. Metadata and fingerprints commit together in batches of up to 100 files. Cancel preserves completed batches and ignores queued work from the canceled scan. Unreadable files produce warnings instead of aborting otherwise valid imports. Unchanged local music files are skipped on later scans.

Sort the library by Album, Title, Artist or Duration. Album order is the default, with stable tie-breaking between pages. Search treats `%` and `_` literally. Favorites, album/artist groups, playlists, play-next insertion and queue reordering are independent of library sorting. Home shows recent listening, or the local library before the first listen.

The player stays available as you browse. Expand it for seeking, lyrics, bookmarks and downloads. Local/downloaded spoken audio supports resume and a sleep timer. Embedded M4A/M4B Nero chapter lists and QuickTime chapter tracks load on demand. Whole-book remote seeking and personal-server bookmark sync remain separate unfinished capabilities.

Audiobooks separates the online LibriVox catalog from local files. Podcasts supports directory search, RSS/Atom feeds, OPML and refreshable subscriptions. Radio supports the public directory, custom stations and M3U/JSON transfer. A delayed catalog request cannot move you away from your current screen. A feed subscription that finishes after navigation is still saved.

Settings is split into Playback and equalizer, Appearance and motion, Files and playlists, Servers and scrobbling, Metadata and lyrics, Sync and listening rooms, and Casting and Discord. Light/dark appearance, sorting and the reduced-motion override persist. The OS motion setting is sampled at startup; selecting Reduce motion disables fades and movement immediately. Follow system honors the platform setting on the next launch too.

Connect optional services in Servers and scrobbling. Subsonic/WebDAV use passwords. Audiobookshelf, ListenBrainz-compatible endpoints and Maloja use API tokens. Last.fm and Libre.fm use JSON containing `api_key`, `api_secret` and an authenticated `session_key`. Invidious needs no secret; the first-run community instance can be removed permanently or replaced. Select music servers in Discover; scrobbling connections are not music catalogs.

Secrets use native secure storage with no plaintext fallback. URLs cannot embed credentials, queries or fragments. Authenticated redirects cannot move to a different host and certificate verification stays enabled. HTTP is allowed for explicitly configured servers, so choose HTTPS when confidentiality is required. Music scrobbling, metadata recognition, Discord and encrypted sharing are opt-in. No account, microphone or output stream is opened at startup; public catalogs are requested when you visit them. There is no telemetry.

Ambient mixes combine generated noise and imported loops. Recording opens the microphone only after an explicit request and needs platform permission. Cast receivers, AirPlay, OS media controls and physical recording require their actual devices for validation. Encrypted sync transfers metadata, not audio files or credentials. Listen Together invitations contain a private room key and should be shared privately.

Use Settings → Appearance and motion → Keyboard shortcuts for navigation keys. Editable fields consume their normal text keys. Escape closes panels, while Ctrl/Command + Q quits through the normal worker cleanup path. `auvemir --help` and `--version` work without a display.
