# Auvemir transformation and verification

The native Rust/Slint app now has a cohesive responsive interface, persistent listening preferences, safer imports and asynchronous navigation, genuine screenshots, reproducible validation and two measured optimization passes. The work is integrated on `Main`. Release signing, physical-device qualification and authenticated live-service checks remain separate requirements.

## 1. Transformation summary

The redesign keeps the existing local audio, personal-server, audiobook, podcast, radio, ambient, recording, lyrics, sync and sharing implementation. It improves the paths people use to import, browse, queue, play and configure audio. No new runtime dependency was added. The database stays at schema version 5.

The completed feature baseline was frozen at `3f2855bacae9f36176eb187eb09c8d86cdf23de8` before either performance pass. The first optimization is preserved in `5d4ccfe0b9ae07d440f5c1f85c4595a35a7a0b7c`; the second is in `4430194c0903ce3e3ccb4ef6c9db3e260ba91a13`. CI portability fixes between the passes address actual platform failures.

## 2. UI/UX redesign

Shared semantic colors provide dark blue-green and warm light appearances, with soft green accents, clear hierarchy, consistent spacing and platform fonts. Home shows the actual library when recent listening is empty. A single sidebar entry per destination, scrollable playlists and fixed Queue/Settings controls keep navigation usable. Phone Home exposes Ambient and Recordings.

Import controls consistently offer files, folders and typed paths. Narrow book/podcast toolbars split discovery from local import. Settings has seven task-based groups. Queue, settings and expanded-player controls remain reachable at the minimum supported width. Native controls expose labels, focus and default actions. See [DESIGN.md](DESIGN.md), [USAGE.md](USAGE.md) and the [real gallery](screenshots/).

## 3. Animation improvements

Headings reserve subtitle space, avoiding layout jumps. Subtitle replacement uses a 90 ms delay, 240 ms opacity transition and 270 ms ease-out movement over five logical pixels. Hover/press feedback uses 130 ms native color transitions. A one-shot timer stops after replacement; rapid navigation selects the latest subtitle.

Reduced motion disables movement and fades. The persistent in-app override takes effect immediately. Bounded startup queries read supported OS preferences; desktop child processes are reaped. Changes to the OS setting require relaunch. The [navigation GIF](screenshots/navigation.gif) records the real implementation and is not an FPS benchmark.

## 4. Advanced features

- Persistent album, title, artist and duration sorting with deterministic ID ties and stable pagination. Queues and playlists retain their intentional order.
- Background import progress and cancellation, bounded per-file warnings and retained completed batches.
- Persistent light appearance and reduced motion, keyboard navigation, search/import/queue/settings shortcuts, a shortcut guide and normal-cleanup quit.
- A music-service selector and persistent removal of the first-run community connection.
- Opt-in native capture controls, responsive/DPI verification and reproducible demo fixtures for maintainers.

## 5. Bugs fixed

Scan batches commit media metadata and fingerprints in one transaction. Invalid timestamps, malformed paths and individual file errors no longer abort other valid files; warning storage is capped. Canceled generations ignore queued batches/completion, and a failed database batch cannot report success.

View generations prevent late catalog results and errors from replacing a newer screen. Completed feed subscriptions still persist after navigation without taking the user back. Invalid station saves retain the editor and show validation errors.

A focus-only layer previously consumed the first click on row actions and Home cards. Focus scopes now sit behind interactive controls and avoid click capture; actual one-click favorite and queue actions pass. Pagination appears only on paginated views.

Public CI exposed a missing Linux `libxkbcommon-x11` runtime, Windows lint imports for Unix-only tests and a Linux-specific malformed-filename fixture running on macOS. Dependencies and platform test gating were corrected. AppImage assembly now includes the dynamically loaded X11 library and its allowed dependencies.

## 6. Architecture improvements

The worker remains the owner of persistent state. Separate scan/view generations isolate canceled work and stale responses without conflating playback or download lifetimes. Atomic batch saving preserves favorites and avoids metadata/fingerprint disagreement.

Artwork now carries immutable Slint pixel buffers from the bounded worker cache to the UI. Mutable consumers use copy-on-write. Cached media queries reuse SQL preparation while reading current database values. The existing bounded caches and connection lifetime manage these resources.

Native credential storage, TLS verification, authenticated redirect isolation, network bounds, codec/DSP behavior, encrypted formats, schema compatibility and public APIs are retained. Audio buffer sizes, timer cadence, encryption work factors and provider protocols were not changed for benchmark gains.

## 7. Extreme optimization results

Both passes followed the verified feature baseline. Raw records, methods, hashes and limitations are in [validation/](validation/).

| Workload, release mode | Before | After | Measured result |
| --- | ---: | ---: | --- |
| Pass 1: 5,000 handoffs of one decoded 192 × 192 RGBA thumbnail, median of seven | 14.660 ms | 0.026 ms | Repeated full-image allocation/copy removed |
| Pass 2: 5,000-track queue restore, five alternating process runs per version, seven samples each | 34.826 ms | 9.746 ms | 3.57× faster; 72.0% lower component latency |
| Final thumbnail workload, median of seven | 14.660 ms baseline | 0.020 ms | Shared-buffer behavior retained |

Pass 1 replaces a pixel copy on every handoff with a shared immutable buffer clone. Handoff work changes from O(width × height) to O(1). The benchmark avoids 737,280,000 bytes of repeated pixel copying across 5,000 handoffs, not that much peak RAM. Decode/cache-miss work and image wrapper overhead remain. Exact pixels, copy-on-write isolation and invalidation are covered.

After pass 1, profiling the queue restore path identified repeated SQL parsing in `Store::get`. Pass 2 uses the existing prepared-statement cache. Indexed lookups, decode work, order, missing-row behavior and returned values stay the same; queue restoration still scales with queue size. Regression coverage checks edits, favorites, deletion, malformed values and reuse after a decode error. The [alternating comparison](validation/queue-paired-pass2.json) controls for host variation better than comparing runs from separate sessions. All outputs retain BLAKE3 `cafaeee05438c36108bf814135c09a6501f4e75a78b7abd50a7a390ebc26843d`.

The full native workload restores a 5,000-track queue, renders through Slint software rendering and exits after a fixed 1,000 ms capture timer. Three isolated warm-cache samples per version produced:

| Native workload median | Feature baseline | Pass 1 | Pass 2 |
| --- | ---: | ---: | ---: |
| Peak process RSS | 67,396 KiB | 50,392 KiB | 50,000 KiB |
| Process CPU, startup/idle/render together | 163.565 ms | 159.299 ms | 158.455 ms |
| Wall time, including fixed timer and PNG encoding | 1,116.830 ms | 1,118.630 ms | 1,146.060 ms |
| Release binary size | 43,902,152 B | 43,902,792 B | 43,902,792 B |

Final peak RSS is 25.8% lower than the feature baseline in this workload. Wall/CPU samples vary and do not establish pure startup, idle, FPS, battery or universal speedups. The extra 640 binary bytes are disclosed. Sorted-page timing was also recorded; its unchanged query is not presented as an optimization result.

Pass 1 measurements were recovered from an earlier completed execution and retain explicit provenance. The final source was freshly rebuilt and rerun here. Final application source SHA-256 is `d5dca5eb1c769ec0235749fd4caa860a08acab2bea6cb8ae9835a88c6c651b34`; binary SHA-256 is `37b302a817fbe56be9cdf712ea86e92badf533a17ebaa68135837f7627e252bb`.

## 8. Testing and verification

| Check | Result and scope |
| --- | --- |
| Rust formatting, release Clippy with warnings denied, release build | Passed |
| Public desktop quality CI on Linux, Windows and macOS | All three passed for both completed optimization passes |
| Release workspace suite | 97 passed, zero failed; one explicit benchmark ignored by the normal suite and run separately |
| Native Linux journeys/render/error checks | All 36 passed |
| Actual encoded codec fixtures | All 14 passed, including optional system FFmpeg fallback cases |
| Static screenshot comparison | All eight match the feature baseline in dimensions and every RGBA byte |
| Large-library capture and queue output | Identical across baseline and both passes |
| UI/animation source after performance work | Identical to the completed feature baseline |
| Linux portable, DEB, RPM, Arch and AppImage packages | All five extracted; executable hashes match the final release |
| AppImage execution | Fixture decoded, native window captured, bundled X11 keyboard library observed loading |

The native suite checks actual import and persistence, one-click favorite/queue actions, null-sink decoding/output activity, preference changes, restart persistence, keyboard responsiveness/clean quit, six viewport/DPI configurations, all destinations/settings at minimum width, expanded player and nonzero capture-write failures. Rendering a page is distinguished from completing its authenticated or hardware workflow.

Package extraction verifies containers, licenses and binary identity. AppImage tools/runtime came from official upstream release assets and their supplied SHA-256 digests were verified. These checks do not install the packages on clean distributions.

Machine-readable evidence: [Rust](validation/rust-final.json), [desktop](validation/desktop-pass2.json), [codecs](validation/codecs.json), [parity](validation/parity-final.json), [packages](linux-build-results.json), [public CI](validation/ci-code.json) and [capture manifest](screenshots/manifest.json). The public [three-platform run for both optimization passes](https://github.com/deactivated0/Auvemir/actions/runs/40337413423) passed. The [quality workflow](https://github.com/deactivated0/Auvemir/actions/workflows/ci.yml) exposes subsequent documentation-head status.

## 9. GitHub improvements

The README explains the native player, setup, supported capabilities, shortcuts, privacy, troubleshooting and contribution path. It includes real desktop/light/phone captures and the native navigation GIF, with truthful demo and platform captions. User, design, build, capture, verification and release guides make the work reproducible.

The repository description and twenty relevant topics were updated. CI runs locked Rust checks on Linux, Windows and macOS; Linux also runs native journeys, codecs and capture artifacts. The Linux release workflow now runs package extraction and AppImage execution checks before uploading artifacts, with package verification/tooling evidence attached. Release/mobile workflows retain separate packaging and signing scopes. Existing reports are labeled historical where appropriate.

## 10. Final repository state

PRs 1 and 2 were merged and their work retained. Before cleanup, every previous branch tip was verified as an ancestor of integrated `Main`, and the history was backed up. The former `main`, two checkpoint branches and two merged work branches were removed. GitHub and the local checkout now have exactly one branch named `Main`; GitHub's default is `Main`. Existing version tags remain intact.

[Branch proof](validation/github-main-branch.jpg) shows the consolidated state. Application and validation changes are committed and pushed through the integrated history; final presentation/evidence updates share the same verified application digest.

## 11. Remaining issues and release requirements

Physical speakers/headphones, microphone recording, casting receivers, secure-store/media-key integration, native portals, authenticated live services, screen readers, dynamic text and Android/iOS device lifecycle need their actual environments. Responsive phone screenshots are Linux renders. OS motion preference changes require restart.

Clean-distribution installation, Linux ARM64/mobile current-source release builds, Windows/macOS signing, Apple notarization/provisioning and production Android identity remain release qualification work. No signed production release was published. Broader feature limitations such as gapless/crossfade, advanced podcast scheduling and some server synchronization are documented in [FEATURES.md](FEATURES.md). See [PLATFORMS.md](PLATFORMS.md) and [RELEASE.md](RELEASE.md) for the concrete release path.
