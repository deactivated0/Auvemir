use auvemir_core::{
    database::Store,
    lyrics::{active_line, parse_lrc},
    model::{Media, MediaKind, Station},
    network::validate_url,
    providers::{export_opml, import_opml, parse_feed},
    queue::{Queue, Repeat},
};

fn track(name: &str) -> Media {
    Media::new(MediaKind::LocalMusic, "test", name, name)
}
#[test]
fn data_survives_restart_with_source_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.sqlite");
    let music = track("same.mp3");
    let book = Media::new(MediaKind::Audiobook, "test", "same.mp3", "same");
    assert_ne!(music.id, book.id);
    {
        let mut s = Store::open(&path).unwrap();
        s.upsert(&[music.clone(), book.clone()]).unwrap();
        s.favorite(&music.id).unwrap();
        s.save_progress(&book.id, 123456).unwrap();
        s.bookmark(&book.id, 120000, "Chapter 2").unwrap();
        s.save_queue(&[music.id.clone(), book.id.clone()], 1)
            .unwrap();
        s.playlist("Road trip", std::slice::from_ref(&music.id))
            .unwrap();
    }
    let s = Store::open(path).unwrap();
    assert_eq!(s.count().unwrap(), 2);
    assert!(s.get(&music.id).unwrap().unwrap().favorite);
    assert_eq!(s.progress(&book.id).unwrap(), 123456);
    assert_eq!(s.bookmarks(&book.id).unwrap()[0].name, "Chapter 2");
    assert_eq!(s.load_queue().unwrap().1, 1);
    assert_eq!(
        s.playlist_items(s.playlists().unwrap()[0].0).unwrap()[0].id,
        music.id
    );
}
#[test]
fn station_edit_and_delete_do_not_touch_public_directory_records() {
    let mut s = Store::memory().unwrap();
    let mut station = Station {
        id: "my-station".into(),
        name: "Local".into(),
        url: "https://example.org/live.mp3".into(),
        country: "Iraq".into(),
        language: "Kurdish".into(),
        tags: "music".into(),
        logo: None,
        custom: true,
        favorite: false,
    };
    s.save_station(&station).unwrap();
    s.favorite(&station.id).unwrap();
    station.name = "Renamed".into();
    station.url = "https://example.org/new.mp3".into();
    s.save_station(&station).unwrap();
    assert!(s.stations().unwrap()[0].favorite);
    assert_eq!(s.get(&station.id).unwrap().unwrap().title, "Renamed");
    let mut public = station.clone();
    public.id = "public".into();
    public.custom = false;
    s.save_station(&public).unwrap();
    assert!(s.delete_station("public").is_err());
    s.delete_station("my-station").unwrap();
    assert!(s.get("my-station").unwrap().is_none());
    assert!(s.get("public").unwrap().is_some());
}
#[test]
fn rescanning_preserves_favorites_and_like_queries_are_literal() {
    let mut s = Store::memory().unwrap();
    let mut m = track("100%_finished");
    s.upsert(&[m.clone(), track("100xxfinished")]).unwrap();
    s.favorite(&m.id).unwrap();
    m.artist = "New artist".into();
    s.upsert(&[m.clone()]).unwrap();
    assert!(s.get(&m.id).unwrap().unwrap().favorite);
    assert_eq!(s.media(None, "%_", 0, false).unwrap().len(), 1);
    assert!(s.media(None, "' OR 1=1 --", 0, false).unwrap().is_empty());
}
#[test]
fn queue_repeat_and_shuffle_preserve_active_source() {
    let mut q = Queue::default();
    assert!(!q.advance(true));
    q.replace((0..20).map(|i| track(&i.to_string())).collect(), 3);
    let id = q.current().unwrap().id.clone();
    q.shuffle(42);
    assert_eq!(q.current().unwrap().id, id);
    q.repeat = Repeat::One;
    assert!(q.advance(true));
    assert_eq!(q.cursor, 3);
    assert!(q.advance(false));
    assert_eq!(q.cursor, 4);
    q.cursor = 19;
    q.repeat = Repeat::Off;
    assert!(!q.advance(true));
    q.repeat = Repeat::All;
    assert!(q.advance(true));
    assert_eq!(q.cursor, 0);
    q.remove(0);
    assert_eq!(q.cursor, 0);
    assert_eq!(q.items.len(), 19);
}
#[test]
fn malicious_urls_are_rejected_but_explicit_local_radio_is_allowed() {
    for url in [
        "file:///etc/passwd",
        "ftp://example.org/music",
        "https://user:secret@example.org/",
        "http://127.0.0.1/",
        "http://[::1]/",
        "http://[::ffff:127.0.0.1]/",
        "http://10.1.2.3/",
        "http://169.254.169.254/",
        "http://100.64.0.1/",
        "http://server.local/",
    ] {
        assert!(validate_url(url, false).is_err(), "{url}");
    }
    assert!(validate_url("https://example.org/music.mp3", false).is_ok());
    assert!(validate_url("http://192.168.1.2:8000/live", true).is_ok());
}
#[test]
fn real_rss_enclosures_and_opml_round_trip() {
    let feed=br#"<?xml version="1.0"?><rss version="2.0"><channel><title>Audio &amp; More</title><link>https://example.org/</link><description>Feed</description><item><guid>episode-1</guid><title>First</title><enclosure url="https://example.org/first.mp3" type="audio/mpeg" length="1000"/></item><item><guid>bad</guid><title>Bad</title><enclosure url="http://127.0.0.1/private.mp3" type="audio/mpeg" length="1"/></item></channel></rss>"#;
    let (title, items) = parse_feed(feed, "https://example.org/feed", MediaKind::Podcast).unwrap();
    assert_eq!(title, "Audio & More");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].location, "https://example.org/first.mp3");
    let feeds = vec![(
        "https://example.org/rss?a=1&b=2".into(),
        "Audio & More".into(),
        MediaKind::Podcast,
    )];
    let opml = export_opml(&feeds);
    assert_eq!(import_opml(&opml).unwrap(), vec![feeds[0].0.clone()]);
    assert!(import_opml("<opml><outline xmlUrl=\"file:///private\"/></opml>").is_err());
}
#[test]
fn lrc_multiple_timestamps_and_malformed_lines() {
    let l = parse_lrc(
        "[ar:Artist]\n[00:01.50][00:03.00]Repeated\n[00:02]Second\n[00:99.0]Bad\n[00:NaN]Bad",
    );
    assert_eq!(l.len(), 3);
    assert_eq!(active_line(&l, 0), None);
    assert_eq!(active_line(&l, 1500), Some(0));
    assert_eq!(active_line(&l, 2500), Some(1));
    assert_eq!(active_line(&l, 9999), Some(2));
}
#[test]
fn migration_rejects_a_future_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.sqlite");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("PRAGMA user_version=999").unwrap();
    drop(c);
    assert!(Store::open(path).is_err());
}
#[test]
fn bookmarks_and_playlist_entries_cascade_on_station_delete() {
    let mut s = Store::memory().unwrap();
    let station = Station {
        id: "station".into(),
        name: "Custom".into(),
        url: "https://example.org/live".into(),
        country: "".into(),
        language: "".into(),
        tags: "".into(),
        logo: None,
        custom: true,
        favorite: false,
    };
    s.save_station(&station).unwrap();
    s.bookmark(&station.id, 400, "Marker").unwrap();
    let p = s
        .playlist("One", std::slice::from_ref(&station.id))
        .unwrap();
    s.delete_station(&station.id).unwrap();
    assert!(s.playlist_items(p).unwrap().is_empty());
    assert!(s.bookmarks(&station.id).unwrap().is_empty());
}
#[test]
fn scanner_uses_bounded_batches_and_skips_unchanged_files() {
    use std::sync::{Arc, atomic::AtomicBool};
    let dir = tempfile::tempdir().unwrap();
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&38u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&44100u32.to_le_bytes());
    wav.extend_from_slice(&88200u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&2u32.to_le_bytes());
    wav.extend_from_slice(&0i16.to_le_bytes());
    for i in 0..203 {
        std::fs::write(dir.path().join(format!("{i}.wav")), &wav).unwrap();
    }
    let mut sizes = Vec::new();
    let mut known = std::collections::HashMap::new();
    auvemir_core::library::scan_batched(
        dir.path(),
        MediaKind::LocalMusic,
        &known.clone(),
        Arc::new(AtomicBool::new(false)),
        None,
        |batch| {
            sizes.push(batch.items.len());
            for (p, size, mtime) in batch.fingerprints {
                known.insert(p, (size, mtime));
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(sizes, vec![100, 100, 3]);
    assert!(
        auvemir_core::library::scan(
            dir.path(),
            MediaKind::LocalMusic,
            &known,
            Arc::new(AtomicBool::new(false))
        )
        .unwrap()
        .items
        .is_empty()
    );
}
