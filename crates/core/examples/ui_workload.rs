use auvemir_core::{
    database::Store,
    model::{Media, MediaKind, MediaSort},
};
use std::time::Instant;
fn main() -> anyhow::Result<()> {
    let size = std::env::args()
        .nth(1)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(5000);
    let mut db = Store::memory()?;
    let items = (0..size)
        .map(|i| {
            let mut m = Media::new(
                MediaKind::LocalMusic,
                "local",
                &format!("/demo/{i}.flac"),
                &format!("Track {i:06}"),
            );
            m.artist = format!("Artist {}", i % 50);
            m.album = format!("Album {}", i % 200);
            m.duration_ms = (60 + i as u64 % 500) * 1000;
            m.track_number = (i % 20) as u32;
            m
        })
        .collect::<Vec<_>>();
    db.upsert(&items)?;
    db.save_queue(
        &items.iter().map(|m| m.id.clone()).collect::<Vec<_>>(),
        size / 2,
    )?;
    let mut samples = Vec::new();
    let mut fingerprint = String::new();
    for _ in 0..7 {
        let started = Instant::now();
        let (ids, cursor) = db.load_queue()?;
        let restored = ids
            .iter()
            .map(|id| db.get(id)?.ok_or_else(|| anyhow::anyhow!("Missing media")))
            .collect::<anyhow::Result<Vec<_>>>()?;
        samples.push(started.elapsed().as_secs_f64() * 1000.);
        assert_eq!(restored, items);
        assert_eq!(cursor, size / 2);
        fingerprint = blake3::hash(&serde_json::to_vec(&(restored, cursor))?)
            .to_hex()
            .to_string();
    }
    let mut page_samples = Vec::new();
    for _ in 0..7 {
        let started = Instant::now();
        for offset in [0, 100, 1000] {
            std::hint::black_box(db.media_sorted(
                None,
                "Track",
                offset,
                false,
                MediaSort::Artist,
            )?);
        }
        page_samples.push(started.elapsed().as_secs_f64() * 1000.);
    }
    println!(
        "{}",
        serde_json::json!({"tracks":size,"queue_restore_ms":samples,"three_sorted_pages_ms":page_samples,"output_blake3":fingerprint})
    );
    Ok(())
}
