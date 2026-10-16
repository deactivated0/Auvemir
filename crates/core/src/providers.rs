use crate::{
    model::{CatalogBook, Media, MediaKind, Station, stable_id},
    network::{text_limited, validate_url},
};
use anyhow::{Result, bail};
use serde::Deserialize;

#[derive(Clone)]
pub struct Providers {
    pub client: reqwest::Client,
}
impl Providers {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: crate::network::client(false)?,
        })
    }
    pub async fn radio(&self, search: &str) -> Result<Vec<Station>> {
        // Fail over among directory servers; playback itself is independent.
        let mut last = None;
        for host in [
            "de1.api.radio-browser.info",
            "nl1.api.radio-browser.info",
            "at1.api.radio-browser.info",
        ] {
            let response = self
                .client
                .get(format!("https://{host}/json/stations/search"))
                .query(&[
                    ("name", search),
                    ("limit", "100"),
                    ("hidebroken", "true"),
                    ("order", "votes"),
                    ("reverse", "true"),
                ])
                .send()
                .await;
            match response {
                Ok(r) if r.status().is_success() => {
                    let raw: Vec<RadioRecord> = r.json().await?;
                    return Ok(raw
                        .into_iter()
                        .filter(|s| validate_url(&s.url_resolved, false).is_ok())
                        .map(|s| Station {
                            id: format!("radio-browser:{}", s.stationuuid),
                            name: s.name,
                            url: s.url_resolved,
                            country: s.country,
                            language: s.language,
                            tags: s.tags,
                            logo: (!s.favicon.is_empty()).then_some(s.favicon),
                            custom: false,
                            favorite: false,
                        })
                        .collect());
                }
                Ok(r) => last = Some(format!("Radio directory: HTTP {}", r.status())),
                Err(e) => last = Some(e.to_string()),
            }
        }
        bail!(
            "{}",
            last.unwrap_or_else(|| "Radio directory unavailable".into())
        )
    }
    pub async fn books(&self, search: &str) -> Result<Vec<CatalogBook>> {
        let mut url = url::Url::parse("https://librivox.org/api/feed/audiobooks/")?;
        url.query_pairs_mut().extend_pairs([
            ("format", "json"),
            ("limit", "50"),
            ("coverart", "1"),
            ("sort_order", "desc"),
        ]);
        if !search.trim().is_empty() {
            url.query_pairs_mut()
                .append_pair("title", &format!("^{}", search.trim()));
        }
        let r = self.client.get(url).send().await?.error_for_status()?;
        let catalog: BookResponse = r.json().await?;
        Ok(catalog
            .books
            .into_iter()
            .filter(|b| validate_url(&b.url_rss, false).is_ok())
            .map(|b| CatalogBook {
                id: b.id,
                title: b.title,
                author: b
                    .authors
                    .into_iter()
                    .map(|a| {
                        format!("{} {}", a.first_name, a.last_name)
                            .trim()
                            .to_owned()
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
                language: b.language,
                feed_url: b.url_rss,
                cover: b.coverart_jpg,
            })
            .collect())
    }
    pub async fn feed(&self, url: &str, kind: MediaKind) -> Result<(String, Vec<Media>)> {
        let bytes = text_limited(&self.client, url, 4 * 1024 * 1024).await?;
        parse_feed(&bytes, url, kind)
    }
    pub async fn podcasts(&self, search: &str) -> Result<Vec<CatalogBook>> {
        let mut url = url::Url::parse("https://itunes.apple.com/search")?;
        url.query_pairs_mut().extend_pairs([
            ("term", search),
            ("media", "podcast"),
            ("entity", "podcast"),
            ("limit", "50"),
        ]);
        let data: serde_json::Value = serde_json::from_slice(
            &text_limited(&self.client, url.as_str(), 4 * 1024 * 1024).await?,
        )?;
        Ok(data["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let feed = p["feedUrl"].as_str()?;
                validate_url(feed, false).ok()?;
                Some(CatalogBook {
                    id: stable_id("podcast-directory", feed),
                    title: p["collectionName"].as_str().unwrap_or("Podcast").into(),
                    author: p["artistName"].as_str().unwrap_or("").into(),
                    language: "Podcast".into(),
                    feed_url: feed.into(),
                    cover: p["artworkUrl600"]
                        .as_str()
                        .or_else(|| p["artworkUrl100"].as_str())
                        .map(str::to_owned),
                })
            })
            .collect())
    }
    pub async fn lyrics(&self, m: &Media) -> Result<String> {
        if m.artist.trim().is_empty() {
            bail!("Artist metadata is required for matching lyrics");
        }
        let r = self
            .client
            .get("https://lrclib.net/api/get")
            .query(&[
                ("track_name", m.title.as_str()),
                ("artist_name", m.artist.as_str()),
                ("album_name", m.album.as_str()),
                ("duration", &(m.duration_ms / 1000).to_string()),
            ])
            .send()
            .await?
            .error_for_status()?;
        let r: Lyrics = r.json().await?;
        r.synced_lyrics
            .or(r.plain_lyrics)
            .ok_or_else(|| anyhow::anyhow!("No lyrics available for this recording"))
    }
}
#[derive(Deserialize)]
struct RadioRecord {
    stationuuid: String,
    name: String,
    url_resolved: String,
    #[serde(default)]
    country: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    tags: String,
    #[serde(default)]
    favicon: String,
}
#[derive(Deserialize)]
struct BookResponse {
    #[serde(default)]
    books: Vec<BookRecord>,
}
#[derive(Deserialize)]
struct BookRecord {
    id: String,
    title: String,
    #[serde(default)]
    authors: Vec<Author>,
    #[serde(default)]
    language: String,
    url_rss: String,
    #[serde(default)]
    coverart_jpg: Option<String>,
}
#[derive(Deserialize)]
struct Author {
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lyrics {
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
}

pub fn parse_feed(bytes: &[u8], url: &str, kind: MediaKind) -> Result<(String, Vec<Media>)> {
    let feed = feed_rs::parser::parse(bytes)?;
    let title = feed
        .title
        .as_ref()
        .map(|s| s.content.clone())
        .unwrap_or_else(|| "Audio feed".into());
    let group = stable_id("feed", url);
    let art = feed.logo.or(feed.icon).map(|i| i.uri);
    let mut items = Vec::new();
    for (i, entry) in feed.entries.into_iter().enumerate() {
        let enclosure = entry
            .links
            .iter()
            .find(|l| l.rel.as_deref() == Some("enclosure"))
            .map(|l| l.href.clone())
            .or_else(|| {
                entry
                    .media
                    .iter()
                    .flat_map(|m| m.content.iter())
                    .find_map(|c| c.url.as_ref().map(|u| u.to_string()))
            });
        let Some(source) = enclosure else {
            continue;
        };
        if validate_url(&source, false).is_err() {
            continue;
        }
        let name = entry
            .title
            .map(|t| t.content)
            .unwrap_or_else(|| format!("Episode {}", i + 1));
        let mut m = Media::new(
            kind,
            if kind == MediaKind::Audiobook {
                "librivox"
            } else {
                "rss"
            },
            &source,
            &name,
        );
        m.id = stable_id(&group, &entry.id);
        m.group_id = group.clone();
        m.album = title.clone();
        m.artist = entry
            .authors
            .first()
            .and_then(|a| a.name.clone())
            .unwrap_or_default();
        m.artwork = art.clone();
        m.track_number = i as u32 + 1;
        m.duration_ms = entry
            .media
            .iter()
            .filter_map(|m| m.duration)
            .next()
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        items.push(m);
        if items.len() >= 500 {
            break;
        }
    }
    if items.is_empty() {
        bail!("This feed contains no playable audio enclosures");
    }
    Ok((title, items))
}

/// A provider must supply capabilities and explicit playback authorization.
/// There is intentionally no fabricated YouTube Music endpoint.
pub trait MusicProvider: Send + Sync {
    fn id(&self) -> &str;
    fn search<'a>(
        &'a self,
        query: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Media>>> + Send + 'a>>;
}

pub fn export_opml(feeds: &[(String, String, MediaKind)]) -> String {
    fn escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><opml version=\"2.0\"><head><title>Auvemir subscriptions</title></head><body>",
    );
    for (url, title, kind) in feeds {
        if *kind == MediaKind::Podcast {
            out.push_str(&format!(
                "<outline type=\"rss\" text=\"{}\" xmlUrl=\"{}\"/>",
                escape(title),
                escape(url)
            ));
        }
    }
    out.push_str("</body></opml>");
    out
}
pub fn import_opml(text: &str) -> Result<Vec<String>> {
    let mut reader = quick_xml::Reader::from_str(text);
    let mut urls = Vec::new();
    loop {
        match reader.read_event()? {
            quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e)
                if e.name().as_ref() == "outline" =>
            {
                for a in e.attributes() {
                    let a = a?;
                    if a.key.as_ref() == "xmlUrl" {
                        let url = a
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)?
                            .into_owned();
                        validate_url(&url, false)?;
                        urls.push(url);
                    }
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    urls.sort();
    urls.dedup();
    Ok(urls)
}
