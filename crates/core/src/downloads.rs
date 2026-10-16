use crate::{
    model::{Media, MediaKind, stable_id},
    network::validate_url,
};
use anyhow::{Result, bail};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    io::AsyncWriteExt,
    sync::{Semaphore, watch},
};

#[derive(Debug, Clone)]
pub struct Progress {
    pub received: u64,
    pub total: Option<u64>,
}
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct PartialInfo {
    validator: Option<String>,
}
#[derive(Clone)]
pub struct Downloader {
    client: reqwest::Client,
    slots: Arc<Semaphore>,
    pub max_bytes: u64,
    allow_local: bool,
}
impl Downloader {
    pub fn personal_server(client: reqwest::Client) -> Self {
        let mut downloader = Self::new(client);
        downloader.allow_local = true;
        downloader
    }
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            slots: Arc::new(Semaphore::new(2)),
            max_bytes: 2 * 1024 * 1024 * 1024,
            allow_local: false,
        }
    }
    pub async fn download(
        &self,
        m: &Media,
        dir: &Path,
        mut cancel: watch::Receiver<bool>,
        progress: impl Fn(Progress) + Send,
    ) -> Result<PathBuf> {
        if !matches!(m.kind, MediaKind::Audiobook | MediaKind::Podcast) {
            bail!("Downloads are available for public-domain books and podcast enclosures");
        }
        if *cancel.borrow() {
            bail!("Download paused");
        }
        let _slot = tokio::select! {s=self.slots.acquire()=>s?,_=cancel.changed()=>bail!("Download paused")};
        let url = validate_url(&m.location, self.allow_local)?;
        tokio::fs::create_dir_all(dir).await?;
        const RESERVE: u64 = 16 * 1024 * 1024;
        if fs2::available_space(dir)? < RESERVE {
            bail!("Not enough free space for a download");
        }
        let key = stable_id("download", &m.id);
        let final_path = dir.join(format!("{key}.audio"));
        let partial = dir.join(format!("{key}.part"));
        let metadata_path = dir.join(format!("{key}.resume.json"));
        let info = tokio::fs::read(&metadata_path)
            .await
            .ok()
            .filter(|b| b.len() <= 4096)
            .and_then(|b| serde_json::from_slice::<PartialInfo>(&b).ok())
            .unwrap_or_default();
        if final_path.is_file() {
            return Ok(final_path);
        }
        let mut received = tokio::fs::metadata(&partial)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if received > self.max_bytes {
            bail!("Partial file exceeds download limit");
        }
        // A range alone cannot prove that the remote file has not changed.
        if info.validator.is_none() {
            received = 0;
        }
        let mut request = self.client.get(url);
        if received > 0 {
            request = request
                .header(reqwest::header::RANGE, format!("bytes={received}-"))
                .header(
                    reqwest::header::IF_RANGE,
                    info.validator.as_deref().unwrap(),
                );
        }
        let response = tokio::select! {r=request.send()=>r?.error_for_status()?,_=cancel.changed()=>bail!("Download paused")};
        let resumed = response.status() == reqwest::StatusCode::PARTIAL_CONTENT && received > 0;
        if response.status() == reqwest::StatusCode::PARTIAL_CONTENT && !resumed {
            bail!("Server returned an unsolicited partial response");
        }
        let validator = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|h| h.to_str().ok())
            .filter(|s| !s.starts_with("W/"))
            .or_else(|| {
                response
                    .headers()
                    .get(reqwest::header::LAST_MODIFIED)
                    .and_then(|h| h.to_str().ok())
            })
            .filter(|s| s.len() <= 1024)
            .map(str::to_owned);
        let mut range_total = None;
        if resumed {
            let range = response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|s| s.to_str().ok())
                .unwrap_or("");
            if !range.starts_with(&format!("bytes {received}-")) {
                bail!("Server returned an invalid resume range");
            }
            range_total = range
                .split_once('/')
                .and_then(|(_, s)| s.parse::<u64>().ok());
            if range_total.is_none() || validator != info.validator {
                bail!("Remote file changed or resume identity could not be verified");
            }
        } else {
            received = 0;
        }
        let total = range_total.or_else(|| {
            response
                .content_length()
                .and_then(|n| n.checked_add(received))
        });
        if total.is_some_and(|n| n > self.max_bytes) {
            bail!("Download exceeds the 2 GiB limit");
        }
        if total.is_some_and(|n| {
            n.saturating_sub(received).saturating_add(RESERVE)
                > fs2::available_space(dir).unwrap_or(0)
        }) {
            bail!("The download will not fit on this disk");
        }
        tokio::fs::write(
            &metadata_path,
            serde_json::to_vec(&PartialInfo { validator })?,
        )
        .await?;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(&partial)
            .await?;
        let mut response = response;
        let mut last = std::time::Instant::now();
        loop {
            let next = tokio::select! {r=response.chunk()=>r?,_=cancel.changed()=>{file.flush().await?;bail!("Download paused; choose Download to resume");}};
            let Some(bytes) = next else {
                break;
            };
            if received.saturating_add(bytes.len() as u64) > self.max_bytes {
                bail!("Download exceeds the size limit");
            }
            file.write_all(&bytes).await?;
            received += bytes.len() as u64;
            if last.elapsed().as_millis() >= 250 {
                if fs2::available_space(dir)? < RESERVE {
                    file.flush().await?;
                    bail!("Download paused because disk space is low");
                }
                progress(Progress { received, total });
                last = std::time::Instant::now();
            }
        }
        if total.is_some_and(|t| t != received) {
            bail!("Incomplete download; partial file retained");
        }
        if received == 0 {
            bail!("Server returned an empty file");
        }
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&partial, &final_path).await?;
        // A manifest verifies the exact bytes retained locally, including resumed data.
        let hash_path = final_path.clone();
        let checksum = tokio::task::spawn_blocking(move || -> Result<String> {
            use std::io::Read;
            let mut f = std::fs::File::open(hash_path)?;
            let mut hasher = blake3::Hasher::new();
            let mut block = [0u8; 65536];
            loop {
                let n = f.read(&mut block)?;
                if n == 0 {
                    break;
                }
                hasher.update(&block[..n]);
            }
            Ok(hasher.finalize().to_hex().to_string())
        })
        .await??;
        tokio::fs::write(
            final_path.with_extension("manifest.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"media_id":m.id,"bytes":received,"blake3":checksum}),
            )?,
        )
        .await?;
        let _ = tokio::fs::remove_file(&metadata_path).await;
        progress(Progress { received, total });
        Ok(final_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    async fn serve(response: &'static [u8]) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 2048];
            let _ = socket.read(&mut request).await;
            socket.write_all(response).await.unwrap();
        });
        format!("http://{addr}/episode.mp3")
    }
    fn downloader() -> Downloader {
        let mut d = Downloader::new(crate::network::client(true).unwrap());
        d.allow_local = true;
        d
    }
    #[tokio::test]
    async fn resumed_download_verifies_identity_and_total_length() {
        let url=serve(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nETag: \"same\"\r\nConnection: close\r\n\r\nDEF").await;
        let m = Media::new(MediaKind::Podcast, "test", &url, "Episode");
        let dir = tempfile::tempdir().unwrap();
        let key = stable_id("download", &m.id);
        tokio::fs::write(dir.path().join(format!("{key}.part")), b"ABC")
            .await
            .unwrap();
        tokio::fs::write(
            dir.path().join(format!("{key}.resume.json")),
            br#"{"validator":"\"same\""}"#,
        )
        .await
        .unwrap();
        let (_tx, rx) = watch::channel(false);
        let path = downloader()
            .download(&m, dir.path(), rx, |_| {})
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(path).await.unwrap(), b"ABCDEF");
        let url=serve(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nETag: \"different\"\r\nConnection: close\r\n\r\nXYZ").await;
        let m = Media::new(MediaKind::Podcast, "test", &url, "Changed");
        let key = stable_id("download", &m.id);
        tokio::fs::write(dir.path().join(format!("{key}.part")), b"ABC")
            .await
            .unwrap();
        tokio::fs::write(
            dir.path().join(format!("{key}.resume.json")),
            br#"{"validator":"\"same\""}"#,
        )
        .await
        .unwrap();
        let (_tx, rx) = watch::channel(false);
        assert!(
            downloader()
                .download(&m, dir.path(), rx, |_| {})
                .await
                .is_err()
        );
        assert_eq!(
            tokio::fs::read(dir.path().join(format!("{key}.part")))
                .await
                .unwrap(),
            b"ABC"
        );
        assert!(!dir.path().join(format!("{key}.audio")).exists());
    }
    #[tokio::test]
    async fn complete_download_is_finalized_and_size_limit_is_enforced() {
        let url =
            serve(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nABCD").await;
        let m = Media::new(MediaKind::Podcast, "test", &url, "Episode");
        let dir = tempfile::tempdir().unwrap();
        let (_tx, rx) = watch::channel(false);
        let path = downloader()
            .download(&m, dir.path(), rx, |_| {})
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(path).await.unwrap(), b"ABCD");
        assert!(
            !dir.path()
                .join(format!("{}.part", stable_id("download", &m.id)))
                .exists()
        );
        let url =
            serve(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nABCD").await;
        let m = Media::new(MediaKind::Podcast, "test", &url, "Too big");
        let (_tx, rx) = watch::channel(false);
        let mut d = downloader();
        d.max_bytes = 3;
        assert!(d.download(&m, dir.path(), rx, |_| {}).await.is_err());
    }
    #[tokio::test]
    async fn truncated_download_never_appears_as_finished() {
        let url =
            serve(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\nABC").await;
        let m = Media::new(MediaKind::Audiobook, "test", &url, "Chapter");
        let dir = tempfile::tempdir().unwrap();
        let (_tx, rx) = watch::channel(false);
        assert!(
            downloader()
                .download(&m, dir.path(), rx, |_| {})
                .await
                .is_err()
        );
        assert!(
            !dir.path()
                .join(format!("{}.audio", stable_id("download", &m.id)))
                .exists()
        );
    }
    #[tokio::test]
    async fn cancelled_or_ineligible_content_is_not_downloaded() {
        let m = Media::new(
            MediaKind::Podcast,
            "test",
            "https://example.org/episode.mp3",
            "Episode",
        );
        let dir = tempfile::tempdir().unwrap();
        let (_tx, rx) = watch::channel(true);
        assert!(
            downloader()
                .download(&m, dir.path(), rx, |_| {})
                .await
                .is_err()
        );
        let m = Media::new(
            MediaKind::OnlineMusic,
            "test",
            "https://example.org/song.mp3",
            "Song",
        );
        let (_tx, rx) = watch::channel(false);
        assert!(
            downloader()
                .download(&m, dir.path(), rx, |_| {})
                .await
                .is_err()
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
