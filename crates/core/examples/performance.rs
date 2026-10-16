//! Deterministic metadata workload; compare this example against an earlier source tree.
use auvemir_core::{
    database::Store,
    model::{Media, MediaKind},
    sync::{Snapshot, Value},
};
use std::time::Instant;

fn canonical(mut snapshot: Snapshot) -> Snapshot {
    let playlists = snapshot
        .records
        .keys()
        .filter(|k| k.starts_with("l:"))
        .cloned()
        .collect::<Vec<_>>();
    for (i, key) in playlists.into_iter().enumerate() {
        let value = snapshot.records.remove(&key).unwrap();
        snapshot.records.insert(format!("l:benchmark-{i}"), value);
    }
    snapshot
}
fn main() -> anyhow::Result<()> {
    let n: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "5000".into())
        .parse()?;
    let mut output = Vec::new();
    for _ in 0..3 {
        let mut db = Store::memory()?;
        let tracks = (0..n)
            .map(|i| {
                let mut m = Media::new(
                    MediaKind::LocalMusic,
                    "local",
                    &format!("/music/{i}.flac"),
                    &format!("Track {i}"),
                );
                m.artist = format!("Artist {}", i % 100);
                m.album = format!("Album {}", i / 10);
                m.duration_ms = 180_000;
                m.favorite = true;
                m
            })
            .collect::<Vec<_>>();
        db.upsert(&tracks)?;
        for (i, m) in tracks.iter().enumerate() {
            db.save_progress(&m.id, i as u64 * 1000 + 1)?;
            if i % 2 == 0 {
                db.attach_download(&m.id, &m.location)?;
            }
        }
        db.playlist(
            "Benchmark",
            &tracks
                .iter()
                .take(1000)
                .map(|m| m.id.clone())
                .collect::<Vec<_>>(),
        )?;
        db.set("sync-device", "0123456789abcdef0123456789abcdef")?;
        let start = Instant::now();
        let first = db.sync_snapshot()?;
        let first_ms = start.elapsed().as_secs_f64() * 1000.;
        let start = Instant::now();
        let steady = db.sync_snapshot()?;
        let steady_ms = start.elapsed().as_secs_f64() * 1000.;
        assert_eq!(first, steady, "Steady sync changed metadata or revisions");
        assert_eq!(
            first
                .records
                .values()
                .filter(|r| matches!(r.value, Value::Favorite(true)))
                .count(),
            n
        );
        let bytes = serde_json::to_vec(&canonical(first))?;
        output.push(serde_json::json!({"tracks":n,"first_sync_ms":first_ms,"steady_sync_ms":steady_ms,"snapshot_blake3":blake3::hash(&bytes).to_hex().as_str()}));
    }
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
