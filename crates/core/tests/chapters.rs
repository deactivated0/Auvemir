use auvemir_core::library::{EmbeddedChapter, embedded_chapters};
use mp4ameta::{Chapter, Tag};
use std::{path::PathBuf, time::Duration};

fn make_book(track: bool, both: bool) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("book.m4b");
    std::fs::write(&path, include_bytes!("fixtures/chapter-base.m4a")).unwrap();
    let mut tag = Tag::read_from_path(&path).unwrap();
    let chapters = [
        Chapter::new(Duration::ZERO, "Opening"),
        Chapter::new(Duration::from_millis(850), "The journey"),
    ];
    if both {
        tag.chapter_list_mut()
            .push(Chapter::new(Duration::ZERO, "List copy"));
    }
    if track {
        tag.chapter_track_mut().extend(chapters);
    } else {
        tag.chapter_list_mut().extend(chapters);
    }
    tag.write_to_path(&path).unwrap();
    (dir, path)
}

#[test]
fn reads_nero_chapter_lists_and_prefers_quicktime_chapter_tracks() {
    let (_list_dir, list_path) = make_book(false, false);
    let (_track_dir, track_path) = make_book(true, true);
    let expected = vec![
        EmbeddedChapter {
            title: "Opening".into(),
            position_ms: 0,
        },
        EmbeddedChapter {
            title: "The journey".into(),
            position_ms: 850,
        },
    ];
    assert_eq!(embedded_chapters(&list_path).unwrap(), expected);
    assert_eq!(embedded_chapters(&track_path).unwrap(), expected);
}

#[test]
fn chapterless_mpeg4_audio_returns_an_empty_list_and_invalid_files_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chapterless.m4b");
    std::fs::write(&path, include_bytes!("fixtures/chapter-base.m4a")).unwrap();
    assert!(embedded_chapters(&path).unwrap().is_empty());

    let invalid = dir.path().join("invalid.m4b");
    std::fs::write(&invalid, b"not an MPEG-4 file").unwrap();
    assert!(embedded_chapters(&invalid).is_err());
}
