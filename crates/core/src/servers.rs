//! Personal server adapters. Catalog identities never contain access tokens.
use crate::{
    model::{Media, MediaKind, stable_id},
    network,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub endpoint: String,
    pub username: String,
}
pub struct Stream {
    pub location: String,
    pub client: reqwest::Client,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemoteProgress {
    pub current_time_ms: u64,
    pub is_finished: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerFolder {
    pub name: String,
    pub path: String,
}
#[derive(Clone, Debug, Default)]
pub struct ServerPage {
    pub items: Vec<Media>,
    pub folders: Vec<ServerFolder>,
    pub has_more: bool,
}
impl Server {
    pub fn validate(&self) -> Result<()> {
        let url = network::validate_url(&self.endpoint, true)?;
        if url.query().is_some() || url.fragment().is_some() {
            bail!("Use a base URL without query parameters or fragments; enter secrets separately");
        }
        if !["subsonic", "audiobookshelf", "webdav", "invidious"].contains(&self.protocol.as_str())
        {
            bail!("Unsupported server protocol");
        }
        if self.name.trim().is_empty() {
            bail!("Give the server a name");
        }
        Ok(())
    }
    fn base(&self) -> Result<Url> {
        self.validate()?;
        Ok(Url::parse(&format!(
            "{}/",
            self.endpoint.trim_end_matches('/')
        ))?)
    }
    fn client(&self, secret: &str) -> Result<reqwest::Client> {
        let mut headers = reqwest::header::HeaderMap::new();
        if self.protocol == "audiobookshelf" {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {secret}").parse()?,
            );
        }
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .user_agent("Auvemir/0.1")
            .connect_timeout(std::time::Duration::from_secs(12))
            .read_timeout(std::time::Duration::from_secs(25))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(client)
    }
    fn subsonic(&self, method: &str, secret: &str) -> Result<Url> {
        let mut url = self.base()?.join(&format!("rest/{method}.view"))?;
        let salt = stable_id(
            "salt",
            &format!(
                "{}:{}",
                crate::scrobble::now(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ),
        );
        let token = format!("{:x}", md5::compute(format!("{secret}{salt}")));
        url.query_pairs_mut().extend_pairs([
            ("u", self.username.as_str()),
            ("t", token.as_str()),
            ("s", salt.as_str()),
            ("v", "1.16.1"),
            ("c", "Auvemir"),
            ("f", "json"),
        ]);
        Ok(url)
    }
    pub async fn search(&self, secret: &str, query: &str) -> Result<Vec<Media>> {
        Ok(self.search_page(secret, query, 0).await?.items)
    }
    /// Return one stable 100-item page and whether the catalog has another page.
    pub async fn search_page(
        &self,
        secret: &str,
        query: &str,
        offset: usize,
    ) -> Result<ServerPage> {
        self.search_directory_page(secret, query, offset, None)
            .await
    }
    pub async fn search_directory_page(
        &self,
        secret: &str,
        query: &str,
        offset: usize,
        webdav_path: Option<&str>,
    ) -> Result<ServerPage> {
        if webdav_path.is_some() && self.protocol != "webdav" {
            bail!("Folder navigation is only available for WebDAV servers");
        }
        match self.protocol.as_str() {
            "invidious" => {
                let client = self.client("")?;
                let mut items = Vec::with_capacity(100);
                let mut has_more = false;
                if query.trim().is_empty() {
                    let mut url = self.base()?.join("api/v1/trending")?;
                    url.query_pairs_mut().append_pair("type", "music");
                    let data = read(client.get(url)).await?;
                    items.extend(invidious_items(&self.id, &data));
                } else {
                    // Invidious returns 20 search results per page; gather five provider
                    // pages for each 100-row Auvemir page to avoid skipping results.
                    let first = (offset / 100).saturating_mul(5).saturating_add(1);
                    for page in first..first.saturating_add(5) {
                        let mut url = self.base()?.join("api/v1/search")?;
                        url.query_pairs_mut().extend_pairs([
                            ("q", query),
                            ("type", "video"),
                            ("page", &page.to_string()),
                        ]);
                        let data = read(client.get(url)).await?;
                        let rows = data.as_array().map_or(0, Vec::len);
                        has_more = rows >= 20;
                        items.extend(invidious_items(&self.id, &data));
                        if items.len() >= 100 {
                            items.truncate(100);
                            break;
                        }
                        if rows < 20 {
                            has_more = false;
                            break;
                        }
                    }
                }
                items.truncate(100);
                Ok(ServerPage {
                    items,
                    folders: Vec::new(),
                    has_more,
                })
            }
            "subsonic" => {
                let mut url = self.subsonic("search3", secret)?;
                url.query_pairs_mut().extend_pairs([
                    ("query", query),
                    ("songCount", "101"),
                    ("songOffset", &offset.to_string()),
                    ("albumCount", "0"),
                    ("artistCount", "0"),
                ]);
                let value = read(self.client(secret)?.get(url)).await?;
                let root = &value["subsonic-response"];
                if root["status"] != "ok" {
                    bail!(
                        "Music server rejected the request; check credentials and API permissions"
                    );
                }
                let songs = root["searchResult3"]["song"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let has_more = songs.len() > 100;
                Ok(ServerPage {
                    items: songs
                        .iter()
                        .take(100)
                        .filter_map(|song| self.song(song))
                        .collect(),
                    folders: Vec::new(),
                    has_more,
                })
            }
            "audiobookshelf" => {
                let client = self.client(secret)?;
                let libraries = read(client.get(self.base()?.join("api/libraries")?)).await?;
                let mut items = Vec::new();
                let target = offset.saturating_add(101);
                let query = query.to_lowercase();
                for library in libraries["libraries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(20)
                {
                    let Some(id) = library["id"].as_str() else {
                        continue;
                    };
                    let mut url = self.base()?;
                    url.path_segments_mut()
                        .map_err(|_| anyhow::anyhow!("Invalid endpoint"))?
                        .pop_if_empty()
                        .extend(["api", "libraries", id, "items"]);
                    let mut page = 0usize;
                    loop {
                        let mut page_url = url.clone();
                        page_url
                            .query_pairs_mut()
                            .extend_pairs([("limit", "100"), ("page", &page.to_string())]);
                        let result = read(client.get(page_url)).await?;
                        let rows = result["results"].as_array().map_or(0, Vec::len);
                        for item in result["results"].as_array().into_iter().flatten() {
                            let metadata = &item["media"]["metadata"];
                            let title = metadata["title"].as_str().unwrap_or("Untitled");
                            if !title.to_lowercase().contains(&query) {
                                continue;
                            }
                            let Some(id) = item["id"].as_str() else {
                                continue;
                            };
                            let mut m = Media::new(
                                MediaKind::Audiobook,
                                &format!("server:{}", self.id),
                                &format!("auvemir://server/{}/{id}", self.id),
                                title,
                            );
                            m.album = title.into();
                            m.artist = metadata["authorName"].as_str().unwrap_or("").into();
                            m.duration_ms =
                                (item["media"]["duration"].as_f64().unwrap_or(0.) * 1000.) as u64;
                            m.group_id = id.into();
                            items.push(m);
                            if items.len() >= target {
                                break;
                            }
                        }
                        if items.len() >= target
                            || rows < 100
                            || result["total"]
                                .as_u64()
                                .is_some_and(|total| (page + 1).saturating_mul(100) as u64 >= total)
                        {
                            break;
                        }
                        page += 1;
                    }
                }
                let has_more = items.len() > offset.saturating_add(100);
                let page = items.into_iter().skip(offset).take(100).collect();
                Ok(ServerPage {
                    items: page,
                    folders: Vec::new(),
                    has_more,
                })
            }
            "webdav" => self.webdav(secret, query, offset, webdav_path).await,
            _ => bail!("Unsupported server"),
        }
    }
    fn song(&self, song: &Value) -> Option<Media> {
        let id = song["id"].as_str()?;
        let mut m = Media::new(
            MediaKind::OnlineMusic,
            &format!("server:{}", self.id),
            &format!("auvemir://server/{}/{id}", self.id),
            song["title"].as_str().unwrap_or("Untitled"),
        );
        m.artist = song["artist"].as_str().unwrap_or("").into();
        m.album = song["album"].as_str().unwrap_or("").into();
        m.duration_ms = song["duration"].as_u64().unwrap_or(0) * 1000;
        m.track_number = song["track"].as_u64().unwrap_or(0) as u32;
        m.group_id = song["albumId"].as_str().unwrap_or("").into();
        Some(m)
    }
    pub async fn resolve(&self, secret: &str, media: &Media) -> Result<Stream> {
        if media.provider != format!("server:{}", self.id) {
            bail!("Media belongs to another server");
        }
        let source = Url::parse(&media.location)?;
        let id = source
            .path_segments()
            .and_then(|mut s| s.nth(1))
            .context("Invalid server media identity")?;
        let client = self.client(secret)?;
        let location = match self.protocol.as_str() {
            "invidious" => {
                if !valid_video_id(id) {
                    bail!("Invalid video identity");
                }
                let info =
                    read(client.get(self.base()?.join(&format!("api/v1/videos/{id}"))?)).await?;
                if info["paid"] == true || info["premium"] == true || info.get("error").is_some() {
                    bail!("This video is restricted or unavailable; Auvemir cannot resolve it");
                }
                let format = info["adaptiveFormats"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|f| {
                        f["type"]
                            .as_str()
                            .is_some_and(|mime| mime.starts_with("audio/mp4"))
                    })
                    .max_by_key(|f| {
                        f["bitrate"]
                            .as_str()
                            .and_then(|s| s.parse::<u64>().ok())
                            .or_else(|| f["bitrate"].as_u64())
                            .unwrap_or(0)
                    })
                    .context("The provider did not offer a supported direct audio stream")?;
                let location = format["url"]
                    .as_str()
                    .context("Provider returned no audio URL")?;
                network::validate_url(location, false)?;
                return Ok(Stream {
                    location: location.into(),
                    client: network::client(false)?,
                });
            }
            "subsonic" => {
                let mut url = self.subsonic("stream", secret)?;
                url.query_pairs_mut().append_pair("id", id);
                url.to_string()
            }
            "audiobookshelf" => {
                let item = self.abs_item(secret, id).await?;
                let index = source
                    .query_pairs()
                    .find(|(k, _)| k == "track")
                    .and_then(|(_, v)| v.parse::<usize>().ok())
                    .unwrap_or(0);
                let track = item["media"]["tracks"]
                    .as_array()
                    .and_then(|a| a.get(index))
                    .context("No playable audio track")?;
                let content = track["contentUrl"].as_str().context("No playback URL")?;
                let url = self.base()?.join(content)?;
                if url.origin() != self.base()?.origin() {
                    bail!("Server returned an audio URL on another origin");
                }
                url.to_string()
            }
            "webdav" => {
                // The source path is separately encoded in catalog identity; never trust another host.
                let href = source
                    .query_pairs()
                    .find(|(k, _)| k == "path")
                    .map(|(_, v)| v.into_owned())
                    .context("Missing WebDAV path")?;
                let url = self.base()?.join(&href)?;
                if url.origin() != self.base()?.origin() {
                    bail!("WebDAV file is on another origin");
                }
                let mut headers = reqwest::header::HeaderMap::new();
                let auth = reqwest::Client::new()
                    .get(url.clone())
                    .basic_auth(&self.username, Some(secret))
                    .build()?;
                if let Some(v) = auth.headers().get(reqwest::header::AUTHORIZATION) {
                    headers.insert(reqwest::header::AUTHORIZATION, v.clone());
                }
                let client = reqwest::Client::builder()
                    .default_headers(headers)
                    .redirect(reqwest::redirect::Policy::none())
                    .read_timeout(std::time::Duration::from_secs(25))
                    .build()?;
                return Ok(Stream {
                    location: url.to_string(),
                    client,
                });
            }
            _ => bail!("Unsupported server"),
        };
        Ok(Stream { location, client })
    }
    async fn abs_item(&self, secret: &str, id: &str) -> Result<Value> {
        self.abs_item_details(secret, id, false).await
    }
    async fn abs_item_details(
        &self,
        secret: &str,
        id: &str,
        include_progress: bool,
    ) -> Result<Value> {
        let mut url = self.base()?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Invalid endpoint"))?
            .pop_if_empty()
            .extend(["api", "items", id]);
        url.query_pairs_mut().append_pair("expanded", "1");
        if include_progress {
            url.query_pairs_mut().append_pair("include", "progress");
        }
        read(self.client(secret)?.get(url)).await
    }
    pub async fn expand_book(&self, secret: &str, media: &Media) -> Result<Vec<Media>> {
        Ok(self.expand_book_with_progress(secret, media).await?.0)
    }
    pub async fn expand_book_with_progress(
        &self,
        secret: &str,
        media: &Media,
    ) -> Result<(Vec<Media>, Option<RemoteProgress>)> {
        if self.protocol != "audiobookshelf" || media.provider != format!("server:{}", self.id) {
            bail!("This is not an Audiobookshelf book");
        }
        let source = Url::parse(&media.location)?;
        let id = source
            .path_segments()
            .and_then(|mut s| s.nth(1))
            .context("Invalid book identity")?;
        let item = self.abs_item_details(secret, id, true).await?;
        let progress = item.get("userMediaProgress").and_then(|progress| {
            Some(RemoteProgress {
                current_time_ms: (progress["currentTime"].as_f64()? * 1000.0) as u64,
                is_finished: progress["isFinished"].as_bool().unwrap_or(false),
            })
        });
        let mut tracks = Vec::new();
        for (index, track) in item["media"]["tracks"]
            .as_array()
            .into_iter()
            .flatten()
            .take(1000)
            .enumerate()
        {
            let mut source = source.clone();
            source
                .query_pairs_mut()
                .append_pair("track", &index.to_string());
            let mut m = Media::new(
                MediaKind::Audiobook,
                &media.provider,
                source.as_str(),
                track["title"].as_str().unwrap_or("Chapter"),
            );
            m.album = media.album.clone();
            m.artist = media.artist.clone();
            m.group_id = media.group_id.clone();
            m.track_number = index as u32 + 1;
            m.duration_ms = (track["duration"].as_f64().unwrap_or(0.) * 1000.) as u64;
            tracks.push(m);
        }
        if tracks.is_empty() {
            bail!("This book has no direct audio tracks");
        }
        Ok((tracks, progress))
    }
    pub async fn update_book_progress(
        &self,
        secret: &str,
        item_id: &str,
        current_time_ms: u64,
        duration_ms: u64,
        is_finished: bool,
    ) -> Result<()> {
        if self.protocol != "audiobookshelf" || item_id.is_empty() {
            bail!("This is not a valid Audiobookshelf book");
        }
        let mut url = self.base()?;
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Invalid endpoint"))?
            .pop_if_empty()
            .extend(["api", "me", "progress", item_id]);
        let current_time = (current_time_ms as f64 / 1000.0).min(duration_ms as f64 / 1000.0);
        let duration = duration_ms as f64 / 1000.0;
        let progress = if is_finished || duration_ms == 0 {
            0.0
        } else {
            (current_time / duration).clamp(0.0, 1.0)
        };
        let response = self
            .client(secret)?
            .patch(url)
            .json(&serde_json::json!({
                "currentTime": current_time,
                "duration": duration,
                "progress": progress,
                "isFinished": is_finished,
            }))
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("Audiobookshelf progress could not be updated"))?;
        if !response.status().is_success() {
            bail!(
                "Audiobookshelf returned HTTP {} while updating progress",
                response.status().as_u16()
            );
        }
        Ok(())
    }
    async fn webdav(
        &self,
        secret: &str,
        query: &str,
        offset: usize,
        webdav_path: Option<&str>,
    ) -> Result<ServerPage> {
        let base = self.base()?;
        let directory = match webdav_path {
            Some(path) => base.join(path)?,
            None => base.clone(),
        };
        if directory.origin() != base.origin() {
            bail!("WebDAV directory is on another origin");
        }
        let mut response=self.client(secret)?.request(reqwest::Method::from_bytes(b"PROPFIND")?,directory.clone()).basic_auth(&self.username,Some(secret)).header("Depth","1").header("Content-Type","application/xml").body("<d:propfind xmlns:d=\"DAV:\"><d:prop><d:displayname/><d:resourcetype/></d:prop></d:propfind>").send().await.map_err(|_|anyhow::anyhow!("WebDAV server could not be reached"))?;
        if !response.status().is_success() {
            bail!("WebDAV returned HTTP {}", response.status().as_u16());
        }
        let mut data = Vec::new();
        while let Some(c) = response.chunk().await? {
            if data.len() + c.len() > 4 * 1024 * 1024 {
                bail!("WebDAV response exceeds limit");
            }
            data.extend(c);
        }
        let mut reader = quick_xml::Reader::from_reader(data.as_slice());
        reader.config_mut().trim_text(true);
        let mut href = String::new();
        let mut name = String::new();
        let mut field = 0;
        let mut folder = false;
        let mut items = Vec::new();
        let mut folders = Vec::new();
        let mut matches = 0usize;
        let query = query.to_lowercase();
        let mut has_more = false;
        loop {
            use quick_xml::events::Event;
            match reader.read_event()? {
                Event::Start(e) => match e.local_name().as_ref() {
                    "response" => {
                        href.clear();
                        name.clear();
                        folder = false;
                    }
                    "href" => field = 1,
                    "displayname" => field = 2,
                    "collection" => folder = true,
                    _ => {}
                },
                Event::Empty(e) if e.local_name().as_ref() == "collection" => folder = true,
                Event::Text(e) => {
                    let text = e
                        .xml_content(quick_xml::XmlVersion::Implicit1_0)
                        .into_owned();
                    if field == 1 {
                        href.push_str(&text);
                    } else if field == 2 {
                        name.push_str(&text);
                    }
                }
                Event::End(e) => {
                    field = 0;
                    if e.local_name().as_ref() == "response" {
                        let url = base.join(&href)?;
                        if url.origin() != base.origin() {
                            bail!("WebDAV returned a resource on another origin");
                        }
                        if folder {
                            if url.path() != directory.path() {
                                let folder_name = if name.is_empty() {
                                    url.path_segments()
                                        .and_then(|segments| {
                                            segments.rev().find(|segment| !segment.is_empty())
                                        })
                                        .or_else(|| {
                                            url.path_segments()
                                                .and_then(|mut segments| segments.next_back())
                                        })
                                        .unwrap_or("Folder")
                                        .trim_end_matches('/')
                                        .to_owned()
                                } else {
                                    name.clone()
                                };
                                if folder_name.to_lowercase().contains(&query) {
                                    let mut path = url.path().to_owned();
                                    if !path.ends_with('/') {
                                        path.push('/');
                                    }
                                    folders.push(ServerFolder {
                                        name: folder_name,
                                        path,
                                    });
                                }
                            }
                            continue;
                        }
                        let path = url.path().to_lowercase();
                        if ![
                            ".mp3", ".flac", ".wav", ".ogg", ".opus", ".m4a", ".m4b", ".aac",
                            ".aiff", ".mp2",
                        ]
                        .iter()
                        .any(|ext| path.ends_with(ext))
                        {
                            continue;
                        }
                        if name.is_empty() {
                            name = url
                                .path_segments()
                                .and_then(|mut s| s.next_back())
                                .unwrap_or("Audio")
                                .into();
                        }
                        if !name.to_lowercase().contains(&query) {
                            continue;
                        }
                        if matches < offset {
                            matches += 1;
                            continue;
                        }
                        if items.len() == 100 {
                            has_more = true;
                            break;
                        }
                        let id = stable_id("webdav", &href);
                        let mut source = Url::parse(&format!("auvemir://server/{}/{id}", self.id))?;
                        source.query_pairs_mut().append_pair("path", &href);
                        items.push(Media::new(
                            MediaKind::OnlineMusic,
                            &format!("server:{}", self.id),
                            source.as_str(),
                            &name,
                        ));
                        matches += 1;
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        Ok(ServerPage {
            items,
            folders,
            has_more,
        })
    }
}
fn invidious_items(server_id: &str, data: &Value) -> Vec<Media> {
    data.as_array()
        .into_iter()
        .flatten()
        .filter_map(|video| {
            if video["paid"] == true || video["premium"] == true {
                return None;
            }
            let id = video["videoId"].as_str()?;
            if !valid_video_id(id) {
                return None;
            }
            let mut media = Media::new(
                MediaKind::OnlineMusic,
                &format!("server:{server_id}"),
                &format!("auvemir://server/{server_id}/{id}"),
                video["title"].as_str().unwrap_or("Untitled"),
            );
            media.artist = video["author"].as_str().unwrap_or("").into();
            media.duration_ms = video["lengthSeconds"].as_u64().unwrap_or(0) * 1000;
            media.artwork = video["videoThumbnails"]
                .as_array()
                .and_then(|thumbnails| {
                    thumbnails
                        .iter()
                        .find(|thumbnail| thumbnail["width"].as_u64().unwrap_or(0) >= 300)
                        .or_else(|| thumbnails.first())
                })
                .and_then(|thumbnail| thumbnail["url"].as_str())
                .filter(|url| network::validate_url(url, false).is_ok())
                .map(str::to_owned);
            Some(media)
        })
        .collect()
}
fn valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
async fn read(request: reqwest::RequestBuilder) -> Result<Value> {
    let mut response = request
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Personal server could not be reached"))?;
    if !response.status().is_success() {
        bail!(
            "Personal server returned HTTP {}",
            response.status().as_u16()
        );
    }
    let mut data = Vec::new();
    while let Some(c) = response.chunk().await? {
        if data.len() + c.len() > 4 * 1024 * 1024 {
            bail!("Personal server response exceeds limit");
        }
        data.extend(c);
    }
    Ok(serde_json::from_slice(&data)?)
}
