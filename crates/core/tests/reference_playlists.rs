use auvemir_core::{
    database::Store,
    metadata::{ReferencePlaylist, ReferenceSong},
    model::{Media, MediaKind},
};
#[test]
fn missing_metadata_is_persistent_and_resolves_when_audio_arrives() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    let playlist = ReferencePlaylist {
        id: "1234".into(),
        provider: "deezer".into(),
        name: "Imported".into(),
        tracks: vec![ReferenceSong {
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            provider_id: "987".into(),
        }],
    };
    let mut db = Store::open(&path).unwrap();
    let report = db.import_reference(&playlist).unwrap();
    assert_eq!((report.matched, report.total), (0, 1));
    drop(db);
    let mut db = Store::open(&path).unwrap();
    assert_eq!(db.reference_missing("deezer:1234").unwrap().len(), 1);
    let mut m = Media::new(MediaKind::LocalMusic, "local", "/music/song.wav", "Song");
    m.artist = "Artist".into();
    db.upsert(&[m.clone()]).unwrap();
    db.reconcile_references().unwrap();
    assert!(db.reference_missing("deezer:1234").unwrap().is_empty());
    assert_eq!(db.playlist_items(report.playlist_id).unwrap()[0].id, m.id);
    db.delete_playlist(report.playlist_id).unwrap();
    db.reconcile_references().unwrap();
    assert!(db.playlists().unwrap().is_empty());
    assert!(db.reference_summaries().unwrap().is_empty());
}
