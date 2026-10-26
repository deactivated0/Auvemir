# Performance implementation and evidence

The optimization changes preserve media values, revisions, settings, protocol formats, crypto parameters and rendering cadence. Queue editing, radio playlists, native metadata corrections and casting relay are separate feature changes. No codec, DSP, capture callback, cryptographic work factor or public API was removed to obtain these results.

## Bottlenecks and changes

1. Sync allocated each revision with `MAX(counter)` over an unindexed log. Initial capture of n records repeatedly scanned a growing table: O(n²). An additive SQLite counter index changes this part to O(n log n), including index maintenance. Existing databases retain schema version 5 and remain readable by older clients.
2. Repeated sync and setting SQL was parsed for every record. Cached prepared statements reuse SQLite parsing while still reading current persistent values. No setting-value cache was introduced.
3. A visible library page issued one download query per row. The batched path returns the same valid stored-path indicators in one parameterized JSON-table query; error fallback preserves individual-query behavior.
4. The second review shortened the lifetimes of media/progress maps and the previous sync snapshot before constructing the final snapshot. This reduces simultaneous allocations without changing traversal order, revision allocation or merge semantics.
5. Each request for a public HTTP client created a fresh client and connection pool. Reusing one public client lets catalog, metadata, artwork, and media requests retain HTTP connections across calls. Requests configured to allow local servers still use an isolated client with the same redirect and DNS policy as before.

## Measured component workload

`crates/core/examples/performance.rs` creates 5,000 deterministic tracks, favorites and progress records, 2,500 download links and one 1,000-item playlist, then measures first and steady-state sync three times. The baseline is saved source version 4. Generated playlist identity alone is normalized; counters, values and a fixed valid device identity are included in the compared snapshot hash. First and steady snapshots must also compare equal within each run.

| Metric | Baseline | Optimized |
|---|---:|---:|
| Median first snapshot | 2,303.62 ms | 221.97 ms |
| Median steady snapshot | 259.68 ms | 203.68 ms |
| Peak process RSS | 28,136 KiB | 24,636 KiB |

The observed first-sync improvement is 10.38×; peak RSS is 12.4% lower. All runs have the same canonical BLAKE3 snapshot hash: `bece7879887ea4490874d7a77029e09044312fa049839c837a4ac8cc64a0de80`. Raw data is in `performance-baseline.json` and `performance-current.json`. Timing excludes compilation. Concurrent build activity can affect timings; these are component measurements, not whole-app or device performance promises.

## Validation and limits

Regression coverage compares batched flags with legacy queries (including duplicate/missing/Unicode IDs and malformed database paths), verifies cached settings see updates/deletion, checks the counter index query plan, and exercises encrypted sync, conflict resolution, tombstones and unresolved media. The recovered workspace has 67 passing Rust tests, including UI attachment replacement and retained Application context lifecycles. Desktop and Android ARM64 Clippy pass; Android Java/DEX and independent focus-event sequence tests pass. Device checks are recorded separately.

Audio buffer sizes, codec output, EQ/speed algorithms, recording behavior, encryption wire format/Argon2 parameters, network authentication/redirect policies and UI timer cadence were intentionally retained. Broader audio-device, battery and mobile lifecycle profiles require platform hardware. No claim of absolute maximum performance or universal speedup is made.

The shared public HTTP client avoids rebuilding connection pools. This change is structural; it does not claim a measured network latency improvement.

## Further review after the first pass

The sync-apply loop now reuses prepared statements for repeated record, favorite, progress, setting and known-media writes, preserving SQL parameters, order and timestamps. A fresh run of the later pass is recorded in `performance-final.json`: median first snapshot 232.70 ms, steady snapshot 146.80 ms, and peak RSS 25,124 KiB. It preserves the same snapshot hash. Native builds and the software emulator were active concurrently, so compare timings as component observations rather than universal device results. Relative to the baseline, this run is 9.90× faster for initial sync, 1.77× faster for steady sync, and uses 10.7% less peak RSS.

Idle scheduling now blocks on commands when no timed work is due. A sleep timer on paused audio wakes at its deadline rather than four times per second. Disabled scrobblers and in-flight submissions no longer cause idle polling. Active audio UI and Cast refresh intervals remain unchanged. Native Android metadata is updated only when metadata changes, audio-focus requests are reused while granted, and import/browser worker threads time out after inactivity. Desktop UI dispatch keeps its original direct weak handle without the Android attachment mutex.

## Checkpoint 22 repeat

A fresh release-mode run of the same deterministic 5,000-track workload produced a 267.28 ms median first sync and a 202.83 ms median steady sync across three trials. This is 8.62× faster on first sync and 1.28× faster on steady sync than the saved baseline. All three snapshots retain the baseline hash. The run did not capture RSS; the separate peak-memory measurements above remain the memory evidence. Raw timings are in `performance-checkpoint22.json`.
