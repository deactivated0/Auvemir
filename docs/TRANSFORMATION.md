# Auvemir transformation and verification

This recovery checkpoint contains the native UI redesign, import and navigation reliability work, documentation and real captures. The complete feature baseline passed release build, formatting, release Clippy, 96 Rust tests and 36 native acceptance/render/error checks. The two post-feature performance passes follow this verified checkpoint. It is not a certified production release.

## 1. Transformation summary
The app retains its Rust/Slint native architecture and existing audio/server functionality. Shared design tokens, useful persistent preferences and safer background work improve everyday listening.

## 2. UI and UX
Light/dark appearance, a simplified sidebar, responsive import/book toolbars, grouped settings, a local-library home view, stable sorting, meaningful loading states and scrollable small-window player controls.

## 3. Motion
A reserved subtitle frame uses a 5 px upward reveal, native opacity animation and short ease-out timing. Hover/press color feedback uses 130 ms. Reduced motion and startup OS preference detection disable movement and fades.

## 4. Features
Persistent album/title/artist/duration sorting, import progress/cancellation, a music-only server selector, persistent removal of the initial connection, keyboard navigation and a shortcut guide.

## 5. Defects addressed
Scan batches now save metadata and fingerprints atomically. Bad files do not abort otherwise valid imports. Stale catalog responses cannot overwrite newer screens; finished subscriptions still persist after navigation. Invalid station saves retain the editor. A focus-only layer no longer consumes the first click on row controls or home cards. Pager controls are limited to paginated views, and station validation errors remain visible in the editor.

## 6. Architecture and safeguards
No new runtime dependency or schema version. Async catalog results carry view generations, scans carry cancellation generations, and capture timers exist only in explicit developer mode. Native credential storage, TLS/redirect controls, codec/DSP code, encrypted formats and bounded network reads are retained.

## 7. Optimization evidence
The verified feature baseline is recorded in validation/components-baseline.json, validation/native-baseline.json and validation/desktop-baseline.json. Median thumbnail handoff workload: 14.660 ms for 5000 decoded 192×192 RGBA handoffs. Median 5000-item queue restore: 30.378 ms. Native 1000 ms capture-window median: 163.565 ms process CPU and 67396 KiB peak RSS. The fixed capture timer is included in wall time, so this is not a pure startup or idle benchmark. Two separate behavior-preserving passes follow; no speedup is claimed at this checkpoint.

## 8. Verification
The release Rust suite has passed 96 tests with one separately runnable benchmark ignored by the normal suite. Formatting, release Clippy with warnings denied and release compilation passed before the latest UI adjustment and are being rerun. Fourteen real generated codec fixtures passed. All 36 final baseline native checks passed, including actual one-click favorite and queue state changes, restart persistence, normal cleanup, all destinations/settings at minimum width and six viewport/DPI configurations. Earlier verification failures were fixed and rerun; they are not counted as passes. Physical audio, microphone, casting, assistive technology, signed releases and native mobile certification remain unverified.

## 9. GitHub presentation
A professional README, real screenshots, a native navigation GIF, user/build/capture/design guides and contribution instructions are included. The description and twenty accurate topics were updated. CI targets Main and runs native Rust checks across desktop systems, plus Linux UI/codec/capture jobs. Historical evidence is labeled separately.

## 10. Repository state
Earlier PRs 1 and 2 were merged. Existing checkpoint branches are ancestors of the integrated history. Main contains a recovery checkpoint; default-branch switching and deletion of the remaining branches are intentionally pending successful final validation and preservation.

## 11. Remaining work
Complete native button/keyboard/persistence journeys, freeze and measure the upgraded baseline, perform and validate both optimization passes, refresh captures, complete package verification, push the final tree, consolidate to exactly one default Main branch and verify current-head CI. Cross-platform hardware, signing and live-account integration checks require their actual environments.
