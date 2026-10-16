use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
#[derive(Clone, Default)]
pub struct Artwork {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}
pub fn cached_path(base: &std::path::Path, url: &str) -> std::path::PathBuf {
    base.join("remote-artwork").join(format!(
        "{}.png",
        auvemir_core::model::stable_id("artwork", url)
    ))
}
pub async fn fetch(
    base: std::path::PathBuf,
    url: String,
    slots: std::sync::Arc<tokio::sync::Semaphore>,
) -> anyhow::Result<()> {
    let _slot = slots.acquire().await?;
    let path = cached_path(&base, &url);
    if path.is_file() {
        return Ok(());
    }
    let data = auvemir_core::network::text_limited(
        &auvemir_core::network::client(false)?,
        &url,
        8 * 1024 * 1024,
    )
    .await?;
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut reader =
            image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(32 * 1024 * 1024);
        reader.limits(limits);
        let thumb = reader.decode()?.thumbnail(512, 512);
        let dir = path.parent().unwrap();
        std::fs::create_dir_all(dir)?;
        let partial = path.with_extension("part");
        thumb.save_with_format(&partial, image::ImageFormat::Png)?;
        std::fs::rename(&partial, &path)?;
        let mut entries = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                m.is_file()
                    .then_some((m.modified().ok(), m.len(), e.path()))
            })
            .collect::<Vec<_>>();
        let mut bytes = entries.iter().map(|e| e.1).sum::<u64>();
        entries.sort_by_key(|e| e.0);
        for (_, size, file) in entries {
            if bytes <= 128 * 1024 * 1024 {
                break;
            }
            if file != path && std::fs::remove_file(file).is_ok() {
                bytes = bytes.saturating_sub(size);
            }
        }
        Ok(())
    })
    .await??;
    Ok(())
}
impl Artwork {
    pub fn image(&self) -> slint::Image {
        if self.width == 0 {
            return slint::Image::default();
        }
        slint::Image::from_rgba8(
            slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                self.pixels.as_slice(),
                self.width,
                self.height,
            ),
        )
    }
}
#[derive(Default)]
pub struct Cache {
    map: HashMap<String, Artwork>,
    order: VecDeque<String>,
}
impl Cache {
    pub fn invalidate(&mut self, path: &str) {
        self.map.remove(path);
        self.order.retain(|p| p != path);
    }
    /// Local thumbnails are decoded on the state worker. At most 32 cached images.
    pub fn get(&mut self, path: Option<&str>) -> Artwork {
        let Some(path) = path else {
            return Artwork::default();
        };
        if path.starts_with("http://") || path.starts_with("https://") {
            return Artwork::default();
        }
        if let Some(a) = self.map.get(path) {
            return a.clone();
        }
        let a = (|| {
            let mut reader = image::ImageReader::open(path)
                .ok()?
                .with_guessed_format()
                .ok()?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(4096);
            limits.max_image_height = Some(4096);
            limits.max_alloc = Some(32 * 1024 * 1024);
            reader.limits(limits);
            let thumb = reader.decode().ok()?.thumbnail(192, 192).into_rgba8();
            Some(Artwork {
                width: thumb.width(),
                height: thumb.height(),
                pixels: Arc::new(thumb.into_raw()),
            })
        })()
        .unwrap_or_default();
        while self.map.len() >= 32 {
            if let Some(key) = self.order.pop_front() {
                self.map.remove(&key);
            }
        }
        self.map.insert(path.to_owned(), a.clone());
        self.order.push_back(path.to_owned());
        a
    }
}
