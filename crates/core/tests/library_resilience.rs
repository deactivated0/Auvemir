use auvemir_core::{
    database::Store,
    library::{self, Scanned},
    model::{Media, MediaKind, MediaSort},
};
use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
};

#[test]
fn scan_batch_is_atomic_and_preserves_favorites() {
    let mut db = Store::memory().unwrap();
    let item = Media::new(MediaKind::LocalMusic, "local", "/demo.flac", "Before");
    db.upsert(std::slice::from_ref(&item)).unwrap();
    db.favorite(&item.id).unwrap();
    let mut updated = item.clone();
    updated.title = "After".into();
    let mut scan = Scanned {
        items: vec![updated],
        fingerprints: vec![(item.location.clone(), 12, u64::MAX)],
        errors: vec![],
    };
    assert!(db.save_scan(&scan).is_err());
    assert_eq!(db.get(&item.id).unwrap().unwrap().title, "Before");
    assert!(db.fingerprints().unwrap().is_empty());
    scan.fingerprints[0].2 = 42;
    db.save_scan(&scan).unwrap();
    let actual = db.get(&item.id).unwrap().unwrap();
    assert_eq!(actual.title, "After");
    assert!(actual.favorite);
    assert_eq!(db.fingerprints().unwrap()[&item.location], (12, 42));
}

#[test]
fn sort_has_stable_pagination_and_literal_search() {
    let mut db = Store::memory().unwrap();
    let items = (0..220)
        .map(|i| {
            let mut m = Media::new(
                MediaKind::LocalMusic,
                "local",
                &format!("/{i:03}.flac"),
                "Same",
            );
            m.album = "Album".into();
            m.artist = "Artist".into();
            m.duration_ms = (220 - i) * 1000;
            m
        })
        .collect::<Vec<_>>();
    db.upsert(&items).unwrap();
    for sort in [
        MediaSort::Album,
        MediaSort::Title,
        MediaSort::Artist,
        MediaSort::Duration,
    ] {
        let all = [0, 100, 200]
            .into_iter()
            .flat_map(|offset| db.media_sorted(None, "", offset, false, sort).unwrap())
            .collect::<Vec<_>>();
        let ids = all
            .iter()
            .map(|m| &m.id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 220);
        if sort == MediaSort::Duration {
            assert!(all.windows(2).all(|p| p[0].duration_ms <= p[1].duration_ms));
        }
    }
    let literal = Media::new(
        MediaKind::LocalMusic,
        "local",
        "/literal.flac",
        "100%_literal",
    );
    db.upsert(std::slice::from_ref(&literal)).unwrap();
    assert_eq!(
        db.media_sorted(None, "%_", 0, false, MediaSort::Title)
            .unwrap(),
        vec![literal]
    );
}

#[cfg(unix)]
#[test]
fn invalid_timestamp_does_not_abort_other_files() {
    use std::fs::{File, FileTimes};
    let dir = tempfile::tempdir().unwrap();
    for name in ["bad.m4a", "good.m4a"] {
        std::fs::copy("tests/fixtures/chapter-base.m4a", dir.path().join(name)).unwrap();
    }
    let file = File::options()
        .write(true)
        .open(dir.path().join("bad.m4a"))
        .unwrap();
    file.set_times(
        FileTimes::new().set_modified(std::time::UNIX_EPOCH - std::time::Duration::from_secs(1)),
    )
    .unwrap();
    let scan = library::scan(
        dir.path(),
        MediaKind::LocalMusic,
        &HashMap::new(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(scan.items.len(), 1);
    assert!(scan.items[0].location.ends_with("good.m4a"));
    assert_eq!(scan.errors.len(), 1);
}

#[cfg(unix)]
#[test]
fn malformed_path_warnings_are_bounded() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let dir = tempfile::tempdir().unwrap();
    for i in 0..80 {
        let mut name = vec![0xff];
        name.extend_from_slice(format!("{i}.flac").as_bytes());
        std::fs::write(dir.path().join(OsString::from_vec(name)), b"bad").unwrap();
    }
    let scan = library::scan(
        dir.path(),
        MediaKind::LocalMusic,
        &HashMap::new(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(scan.errors.len(), 50);
    assert!(scan.items.is_empty());
}
