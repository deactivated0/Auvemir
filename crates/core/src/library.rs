use crate::model::{Media, MediaKind, stable_id};
use anyhow::{Context, Result};
use lofty::{prelude::*, probe::Probe};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::UNIX_EPOCH,
};

pub struct Scanned {
    pub items: Vec<Media>,
    pub fingerprints: Vec<(String, u64, u64)>,
    pub errors: Vec<String>,
}

/// Scans on a worker; unchanged files are not opened. Symlinks are not followed.
pub fn scan(
    root: &Path,
    kind: MediaKind,
    known: &HashMap<String, (u64, u64)>,
    cancel: Arc<AtomicBool>,
) -> Result<Scanned> {
    let mut all = Scanned {
        items: vec![],
        fingerprints: vec![],
        errors: vec![],
    };
    scan_batched(root, kind, known, cancel, None, |batch| {
        all.items.extend(batch.items);
        all.fingerprints.extend(batch.fingerprints);
        all.errors.extend(batch.errors);
        Ok(())
    })?;
    Ok(all)
}
pub fn scan_batched(
    root: &Path,
    kind: MediaKind,
    known: &HashMap<String, (u64, u64)>,
    cancel: Arc<AtomicBool>,
    artwork_dir: Option<&Path>,
    mut emit: impl FnMut(Scanned) -> Result<()>,
) -> Result<()> {
    let root = root.canonicalize().context("Cannot open import path")?;
    let mut result = Scanned {
        items: vec![],
        fingerprints: vec![],
        errors: vec![],
    };
    let entries: Box<dyn Iterator<Item = Result<PathBuf, walkdir::Error>>> = if root.is_file() {
        Box::new(std::iter::once(Ok(root)))
    } else {
        Box::new(
            walkdir::WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter(|e| e.as_ref().map_or(true, |e| e.file_type().is_file()))
                .map(|e| e.map(|e| e.into_path())),
        )
    };
    for path in entries {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let path = match path {
            Ok(path) => path,
            Err(error) => {
                if result.errors.len() < 50 {
                    result.errors.push(error.to_string());
                }
                continue;
            }
        };
        if !audio_extension(&path) {
            continue;
        }
        // Reject non-UTF-8 paths instead of silently corrupting their persistent identity.
        let Some(location) = path.to_str().map(str::to_owned) else {
            if result.errors.len() < 50 {
                result.errors.push("Skipped a non-UTF-8 path".into());
            }
            continue;
        };
        let metadata = (|| -> Result<_> {
            let meta = std::fs::metadata(&path)?;
            let modified = meta
                .modified()?
                .duration_since(UNIX_EPOCH)?
                .as_nanos()
                .try_into()?;
            Ok((meta, modified))
        })();
        let (meta, modified) = match metadata {
            Ok(value) => value,
            Err(error) => {
                if result.errors.len() < 50 {
                    result.errors.push(format!("{}: {error}", path.display()));
                }
                continue;
            }
        };
        if known.get(&location) == Some(&(meta.len(), modified)) {
            continue;
        }
        match read_media_internal(&path, kind, artwork_dir) {
            Ok(m) => {
                result.items.push(m);
                result.fingerprints.push((location, meta.len(), modified));
            }
            Err(e) => {
                if result.errors.len() < 50 {
                    result.errors.push(format!("{}: {e}", path.display()));
                }
            }
        }
        if result.items.len() >= 100 {
            emit(result)?;
            result = Scanned {
                items: vec![],
                fingerprints: vec![],
                errors: vec![],
            };
        }
    }
    if !cancel.load(Ordering::Relaxed) && (!result.items.is_empty() || !result.errors.is_empty()) {
        emit(result)?;
    }
    Ok(())
}
pub fn audio_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "mp3"
                | "flac"
                | "wav"
                | "ogg"
                | "opus"
                | "aac"
                | "m4a"
                | "m4b"
                | "alac"
                | "aiff"
                | "aif"
                | "mp2"
                | "wma"
                | "ape"
                | "wv"
                | "dsf"
                | "caf"
        )
    })
}
pub fn read_media(path: &Path, kind: MediaKind) -> Result<Media> {
    read_media_internal(path, kind, None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedChapter {
    pub title: String,
    pub position_ms: u64,
}

/// Reads only the embedded chapter list and chapter track from an MPEG-4 audio file.
/// Artwork and unrelated metadata are excluded to keep this on-demand operation bounded.
pub fn embedded_chapters(path: &Path) -> Result<Vec<EmbeddedChapter>> {
    let config = mp4ameta::ReadConfig {
        read_chapter_list: true,
        read_chapter_track: true,
        ..mp4ameta::ReadConfig::NONE
    };
    let tag = mp4ameta::Tag::read_with_path(path, &config)
        .with_context(|| format!("Could not read MPEG-4 chapters from {}", path.display()))?;
    let chapters = if tag.chapter_track().is_empty() {
        tag.chapter_list()
    } else {
        tag.chapter_track()
    };
    anyhow::ensure!(
        chapters.len() <= u16::MAX as usize,
        "MPEG-4 file contains too many chapters"
    );
    Ok(chapters
        .iter()
        .map(|chapter| EmbeddedChapter {
            title: chapter.title.clone(),
            position_ms: u64::try_from(chapter.start.as_millis()).unwrap_or(u64::MAX),
        })
        .collect())
}

fn read_media_internal(path: &Path, kind: MediaKind, artwork_dir: Option<&Path>) -> Result<Media> {
    let location = path.to_str().context("File path is not UTF-8")?;
    let fallback = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");
    let mut m = Media::new(kind, "local", location, fallback);
    // Probe sniffs the actual container; the extension is only a scan filter.
    let file = Probe::open(path)?.guess_file_type()?.read()?;
    m.duration_ms = file
        .properties()
        .duration()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
        m.title = tag
            .title()
            .map(|s| s.into_owned())
            .unwrap_or_else(|| fallback.to_owned());
        m.artist = tag.artist().map(|s| s.into_owned()).unwrap_or_default();
        m.album = tag.album().map(|s| s.into_owned()).unwrap_or_default();
        m.track_number = tag.track().unwrap_or(0);
        if let (Some(dir), Some(picture)) = (artwork_dir, tag.pictures().first())
            && picture.data().len() <= 4 * 1024 * 1024
        {
            std::fs::create_dir_all(dir)?;
            let cover = dir.join(format!("{}.image", blake3::hash(picture.data()).to_hex()));
            if !cover.exists() {
                std::fs::write(&cover, picture.data())?;
            }
            m.artwork = Some(cover.to_string_lossy().into_owned());
        }
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    m.group_id = if kind.long_form() {
        stable_id("book-folder", &parent.to_string_lossy())
    } else {
        stable_id("local-album", &format!("{}\0{}", m.artist, m.album))
    };
    if m.artwork.is_none() {
        m.artwork = ["cover.jpg", "folder.jpg", "cover.png", "folder.png"]
            .iter()
            .map(|n| parent.join(n))
            .find(|p| p.is_file())
            .map(|p| p.to_string_lossy().into_owned());
    }
    Ok(m)
}

pub fn read_m3u(path: &Path, kind: MediaKind) -> Result<Vec<Media>> {
    let text = std::fs::read_to_string(path)?;
    let base = path.parent().unwrap_or(Path::new("."));
    let mut items = Vec::new();
    let mut name = None;
    for line in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            name = info.split_once(',').map(|(_, s)| s.to_owned());
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let mut m = if line.starts_with("https://") || line.starts_with("http://") {
            crate::network::validate_url(line, false)?;
            Media::new(kind, "m3u", line, name.as_deref().unwrap_or("Stream"))
        } else {
            read_media(&base.join(line).canonicalize()?, kind)?
        };
        if let Some(title) = name.take() {
            m.title = title;
        }
        items.push(m);
    }
    Ok(items)
}
pub fn write_m3u(items: &[Media]) -> String {
    let mut s = String::from("#EXTM3U\n");
    for m in items {
        s.push_str(&format!(
            "#EXTINF:{},{}\n{}\n",
            m.duration_ms / 1000,
            m.title.replace(['\r', '\n'], " "),
            m.location.replace(['\r', '\n'], "")
        ));
    }
    s
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Tags {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub composer: String,
}
pub fn capture_lock(file: &std::fs::File) -> Result<()> {
    use fs2::FileExt;
    file.try_lock_exclusive()?;
    Ok(())
}
pub fn read_tags(path: &Path) -> Result<Tags> {
    let file = Probe::open(path)?.guess_file_type()?.read()?;
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else {
        return Ok(Tags {
            title: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .into(),
            ..Default::default()
        });
    };
    Ok(Tags {
        title: tag.title().map(|s| s.into_owned()).unwrap_or_default(),
        artist: tag.artist().map(|s| s.into_owned()).unwrap_or_default(),
        album: tag.album().map(|s| s.into_owned()).unwrap_or_default(),
        genre: tag.genre().map(|s| s.into_owned()).unwrap_or_default(),
        composer: tag
            .get_string(lofty::tag::ItemKey::Composer)
            .unwrap_or("")
            .into(),
    })
}
/// Write a complete tag to a sibling copy, then atomically replace the original.
/// Other tags, artwork and audio packets are retained by Lofty's container writer.
pub fn write_tags(path: &Path, tags: &Tags) -> Result<()> {
    use anyhow::{bail, ensure};
    use std::io::{Read, Write};
    ensure!(!tags.title.trim().is_empty(), "Track title is required");
    for text in [
        &tags.title,
        &tags.artist,
        &tags.album,
        &tags.genre,
        &tags.composer,
    ] {
        ensure!(
            text.len() <= 8192 && !text.contains('\0'),
            "Tag text is invalid or too long"
        );
    }
    let mut file = Probe::open(path)?.guess_file_type()?.read()?;
    let tag_type = file.primary_tag_type();
    if file.primary_tag().is_none() {
        file.insert_tag(lofty::tag::Tag::new(tag_type));
    }
    let tag = file
        .primary_tag_mut()
        .context("This format does not support editable tags")?;
    tag.set_title(tags.title.trim().to_owned());
    tag.set_artist(tags.artist.clone());
    tag.set_album(tags.album.clone());
    for (key, value) in [
        (lofty::tag::ItemKey::Genre, &tags.genre),
        (lofty::tag::ItemKey::Composer, &tags.composer),
    ] {
        tag.remove_key(key);
        if !value.is_empty() {
            tag.insert_text(key, value.clone());
        }
    }
    let parent = path
        .parent()
        .context("Audio file has no parent directory")?;
    let size = std::fs::metadata(path)?.len();
    ensure!(
        fs2::available_space(parent)? > size.saturating_add(16 * 1024 * 1024),
        "Insufficient free storage to edit tags safely"
    );
    let temp = parent.join(format!(".auvemir-tags-{:016x}.tmp", rand::random::<u64>()));
    let result = (|| -> Result<()> {
        let mut input = std::fs::File::open(path)?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let mut buffer = [0u8; 65536];
        loop {
            let n = input.read(&mut buffer)?;
            if n == 0 {
                break;
            };
            output.write_all(&buffer[..n])?;
        }
        output.set_permissions(input.metadata()?.permissions())?;
        output.sync_all()?;
        drop(output);
        drop(input);
        tag.save_to_path(&temp, lofty::config::WriteOptions::default())?;
        let output = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&temp)?;
        output.sync_all()?;
        drop(output);
        if !temp.is_file() {
            bail!("Tagged file is unavailable");
        }
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}
pub fn embedded_lyrics(path: &Path) -> Result<Option<String>> {
    let file = Probe::open(path)?.guess_file_type()?.read()?;
    Ok(file
        .tags()
        .iter()
        .find_map(|tag| {
            tag.get_string(lofty::tag::ItemKey::Lyrics)
                .or_else(|| tag.get_string(lofty::tag::ItemKey::UnsyncLyrics))
        })
        .filter(|s| s.len() <= 1024 * 1024)
        .map(str::to_owned))
}
