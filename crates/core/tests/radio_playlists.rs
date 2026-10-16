use auvemir_core::{
    database::Store,
    radio::{parse_m3u, write_m3u},
};
#[test]
fn stations_round_trip_and_preserve_favorites_after_reimport() {
    let input = "\u{feff}#EXTM3U\r\n#EXTINF:-1,إذاعة, live\r\nhttps://example.org/radio?quality=1\r\nhttp://192.168.1.5:8000/live\n";
    let stations = parse_m3u(input).unwrap();
    assert_eq!(stations.len(), 2);
    assert_eq!(stations[0].name, "إذاعة, live");
    assert_eq!(stations[1].name, "192.168.1.5");
    assert_eq!(parse_m3u(&write_m3u(&stations)).unwrap(), stations);
    let mut db = Store::memory().unwrap();
    db.save_station(&stations[0]).unwrap();
    db.favorite(&stations[0].id).unwrap();
    db.save_station(&stations[0]).unwrap();
    assert!(db.stations().unwrap()[0].favorite);
}
#[test]
fn radio_import_rejects_local_files_and_credentials() {
    for input in [
        "file:///etc/passwd",
        "/music/song.mp3",
        "https://name:secret@example.org/live",
        "javascript:alert(1)",
    ] {
        assert!(parse_m3u(input).is_err(), "{input}");
    }
    assert!(parse_m3u("#EXTM3U\n#comment\n").unwrap().is_empty());
}
