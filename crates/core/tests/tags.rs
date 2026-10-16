use auvemir_core::library::{Tags, read_tags, write_tags};
fn wave() -> Vec<u8> {
    let samples = 4096u32;
    let size = samples * 2;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((size + 36).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(44100u32.to_le_bytes());
    b.extend(88200u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(size.to_le_bytes());
    for i in 0..samples {
        b.extend((i as i16).wrapping_mul(17).to_le_bytes());
    }
    b
}
fn pcm(b: &[u8]) -> &[u8] {
    let mut i = 12;
    while i + 8 <= b.len() {
        let n = u32::from_le_bytes(b[i + 4..i + 8].try_into().unwrap()) as usize;
        if &b[i..i + 4] == b"data" {
            return &b[i + 8..i + 8 + n];
        }
        i += 8 + n + (n & 1);
    }
    panic!("No audio data")
}
#[test]
fn atomic_tag_edit_preserves_samples_and_unedited_fields() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("song.wav");
    let original = wave();
    std::fs::write(&path, &original).unwrap();
    let tags = Tags {
        title: "Song".into(),
        artist: "Artist".into(),
        album: "Album".into(),
        genre: "Ambient".into(),
        composer: "Composer".into(),
    };
    write_tags(&path, &tags).unwrap();
    assert_eq!(pcm(&original), pcm(&std::fs::read(&path).unwrap()));
    let actual = read_tags(&path).unwrap();
    assert_eq!(actual, tags);
    let mut changed = actual;
    changed.title = "Revised".into();
    write_tags(&path, &changed).unwrap();
    assert_eq!(read_tags(&path).unwrap().composer, "Composer");
}
#[test]
fn invalid_tag_edit_leaves_file_untouched() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("song.wav");
    let original = wave();
    std::fs::write(&path, &original).unwrap();
    assert!(write_tags(&path, &Tags::default()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
}
