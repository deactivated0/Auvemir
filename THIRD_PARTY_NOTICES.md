# Third-party notices

Auvemir's authored Rust and Slint application code is GPL-3.0-only. No application source or audio/art assets were copied from MetroList, Auxio, RadioDroid, AntennaPod, Voice, BookPlayer, Noice, Fossify, or Audiobookshelf. They informed the requested product behavior; this implementation uses independent Rust modules.

Direct dependencies and their exact transitive versions are recorded in Cargo.lock. A machine-readable dependency license inventory, including materialized platform-only dependency sources, accompanies this source release in `docs/dependency-licenses.json`. This inventory describes declared package licenses; it is not a certification of compatibility or redistribution rights for every possible optional feature.

Notable dependencies:

| Dependency | Role | Declared license |
|---|---|---|
| Slint and slint-build | UI, compiler, software renderer | GPL-3.0-only OR Slint alternative licenses; GPL-3.0-only selected |
| Rodio / CPAL | Playback mixer / native devices | MIT OR Apache-2.0 / Apache-2.0 |
| Symphonia | Decode/demux | MPL-2.0 |
| opus / opusic-sys / libopus | Opus bindings / bundled C decoder | MIT OR Apache-2.0 / BSD-3-Clause and applicable upstream notices |
| rusqlite / SQLite | Persistent data | MIT / public-domain SQLite |
| Tokio / Reqwest / Serde | Runtime, networking, serialization | MIT / MIT OR Apache-2.0 / MIT OR Apache-2.0 |
| Lofty | Metadata | MIT OR Apache-2.0 |
| mp4ameta 0.13.0 | On-demand MPEG-4 audiobook chapter lists and tracks | MIT OR Apache-2.0; upstream notices are in `third_party/mp4ameta/` |
| feed-rs / quick-xml | Feed/OPML parsing | MIT / MIT |
| stream-download | Bounded encoded stream buffer | MIT OR Apache-2.0 |
| Image / Hound | Artwork / WAV recording | MIT OR Apache-2.0 / Apache-2.0 |
| rfd | Native file dialogs | MIT |
| soundtouch / soundtouch-ffi / SoundTouch | Producer-side pitch-preserving speed | LGPL-2.1 |
| keyring / platform keyring stores | Native credentials | MIT OR Apache-2.0; see inventory |
| souvlaki | Native media controls | MIT |
| fs2 / md5 | Disk-space checks / API protocol signatures | MIT OR Apache-2.0 / MIT |
| Argon2 / chacha20poly1305 | Encrypted sync | MIT OR Apache-2.0 |
| rustls / mdns-sd | Cast transport/discovery | Apache-2.0 OR ISC OR MIT / Apache-2.0 OR MIT |
| rusty-chromaprint | Native audio fingerprints | MIT |
| webbrowser / winresource | Explicit external links / Windows resources | MIT OR Apache-2.0 / MIT |

Each upstream copyright notice and license must accompany any redistribution requiring it. Use the dependency inventory and the actual packaged upstream LICENSE/COPYING files when preparing binary releases. `cargo vendor --locked` provides the exact Cargo source tree; keep bundled libopus's notices and code with it. Slint's other license options are not selected by this project.

The service adapters use Radio Browser, LibriVox and LRCLIB. Their data/content terms are separate from this source-code license. Downloads are user-initiated; no remote media is bundled, redistributed, or uploaded. The reference screenshots are user-supplied design inputs and are not runtime assets.

References:

- <https://docs.slint.dev/latest/docs/rust/slint/>
- <https://github.com/slint-ui/slint>
- <https://github.com/RustAudio/rodio>
- <https://github.com/pdeljanov/Symphonia>
- <https://github.com/SpaceManiac/opus-rs>
- <https://opus-codec.org/>
- <https://docs.radio-browser.info/>
- <https://librivox.org/api/info>
- <https://lrclib.net/docs>

SoundTouch 2.3.2's complete bundled C++ sources and license are retained in the corresponding-source dependency tree. Sources can be rebuilt with a modified library; the application is provided under GPL-3.0-only. The optional FFmpeg executable is a separately installed system program and is not bundled.

Windows GNU packages include MinGW/GCC runtime DLLs. Their upstream notices and GCC Runtime Library Exception are included as `THIRD-PARTY-MINGW.txt` in the package and `packaging/native-notices/mingw-runtime.txt` in source. These components retain their original licenses.

AppImage uses an upstream type-2 runtime and selected unmodified, replaceable system shared libraries. Their own license/copyright notices apply. Tool/runtime provenance is recorded in the release build data; do not infer that these components use Auvemir's license.

Additional protocol/documentation references: <https://opensubsonic.netlify.app/docs/api-reference/>, <https://api.audiobookshelf.org/>, <https://docs.invidious.io/api/>, <https://listenbrainz.readthedocs.io/en/latest/users/api/core.html>, <https://www.last.fm/api/show/track.scrobble>, and <https://github.com/Cyanistic/soundtouch>.

The small `crates/android-context` fork preserves ndk-context 0.1.1 APIs and adds a retained Application context for Android foreground audio before and after Activity creation. Upstream authors: The Rust Windowing contributors. Its MIT and Apache-2.0 licenses are included in that directory. `third_party/mp4ameta/` vendors the 0.13.0 MIT OR Apache-2.0 source; its only local change is a compiler-warning allowance with no runtime effect.
