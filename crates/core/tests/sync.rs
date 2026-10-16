use auvemir_core::{
    database::Store,
    model::{Media, MediaKind},
    sync::*,
};
fn track(path: &str) -> Media {
    let mut m = Media::new(MediaKind::Audiobook, "local", path, "Shared chapter");
    m.artist = "Reader".into();
    m.duration_ms = 500_000;
    m
}
#[test]
fn encrypted_round_trip_rejects_wrong_password_and_damage() {
    let mut db = Store::memory().unwrap();
    let m = track("/private/audio.wav");
    db.upsert(std::slice::from_ref(&m)).unwrap();
    db.favorite(&m.id).unwrap();
    let snapshot = db.sync_snapshot().unwrap();
    let bytes = encrypt(&snapshot, "a sufficiently long passphrase").unwrap();
    assert_eq!(
        decrypt(&bytes, "a sufficiently long passphrase").unwrap(),
        snapshot
    );
    assert!(decrypt(&bytes, "a different long passphrase").is_err());
    let mut corrupt = bytes;
    let last = corrupt.len() - 1;
    corrupt[last] ^= 1;
    assert!(decrypt(&corrupt, "a sufficiently long passphrase").is_err());
    assert!(encrypt(&snapshot, "short").is_err());
    assert!(
        !serde_json::to_string(&snapshot)
            .unwrap()
            .contains("/private/")
    );
}
#[test]
fn favorites_progress_and_playlist_tombstones_cross_paths() {
    let mut a = Store::memory().unwrap();
    let mut b = Store::memory().unwrap();
    let one = track("/one/chapter.wav");
    let two = track("D:/two/chapter.wav");
    a.upsert(std::slice::from_ref(&one)).unwrap();
    b.upsert(std::slice::from_ref(&two)).unwrap();
    a.favorite(&one.id).unwrap();
    a.save_progress(&one.id, 123_456).unwrap();
    let list = a
        .playlist("My book", std::slice::from_ref(&one.id))
        .unwrap();
    let sa = a.sync_snapshot().unwrap();
    b.sync_snapshot().unwrap();
    b.sync_merge(&sa).unwrap();
    assert!(b.get(&two.id).unwrap().unwrap().favorite);
    assert_eq!(b.progress(&two.id).unwrap(), 123_456);
    let bl = b.playlists().unwrap()[0].0;
    assert_eq!(b.playlist_items(bl).unwrap()[0].id, two.id);
    b.favorite(&two.id).unwrap();
    a.sync_merge(&b.sync_snapshot().unwrap()).unwrap();
    assert!(!a.get(&one.id).unwrap().unwrap().favorite);
    a.delete_playlist(list).unwrap();
    b.sync_merge(&a.sync_snapshot().unwrap()).unwrap();
    assert!(b.playlists().unwrap().is_empty());
}
#[test]
fn missing_audio_resolves_without_losing_remote_values() {
    let mut a = Store::memory().unwrap();
    let mut b = Store::memory().unwrap();
    let one = track("/one.wav");
    a.upsert(std::slice::from_ref(&one)).unwrap();
    a.favorite(&one.id).unwrap();
    a.playlist("Offline later", std::slice::from_ref(&one.id))
        .unwrap();
    let remote = a.sync_snapshot().unwrap();
    b.sync_merge(&remote).unwrap();
    let pending = b.sync_snapshot().unwrap();
    assert_eq!(
        pending
            .records
            .get(&format!("f:{}", identity(&one)))
            .unwrap()
            .value,
        Value::Favorite(true)
    );
    let two = track("/two.wav");
    b.upsert(std::slice::from_ref(&two)).unwrap();
    b.sync_snapshot().unwrap();
    assert!(b.get(&two.id).unwrap().unwrap().favorite);
    assert_eq!(
        b.playlist_items(b.playlists().unwrap()[0].0).unwrap().len(),
        1
    );
}
#[test]
fn merge_is_commutative_and_rejects_secret_settings() {
    let mut a = Snapshot::default();
    let mut b = Snapshot::default();
    let k = format!("f:{}", "a".repeat(64));
    a.records.insert(
        k.clone(),
        Record {
            counter: 2,
            device: "1".repeat(32),
            value: Value::Favorite(true),
        },
    );
    b.records.insert(
        k,
        Record {
            counter: 2,
            device: "2".repeat(32),
            value: Value::Favorite(false),
        },
    );
    let mut x = a.clone();
    x.merge(&b).unwrap();
    let mut y = b.clone();
    y.merge(&a).unwrap();
    assert_eq!(x, y);
    x.merge(&y).unwrap();
    assert_eq!(x, y);
    a.records.insert(
        "s:password".into(),
        Record {
            counter: 3,
            device: "1".repeat(32),
            value: Value::Setting("secret".into()),
        },
    );
    assert!(a.validate().is_err());
}
