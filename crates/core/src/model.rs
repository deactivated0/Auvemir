use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    LocalMusic,
    OnlineMusic,
    Audiobook,
    Podcast,
    Radio,
    Ambient,
    Recording,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalMusic => "local_music",
            Self::OnlineMusic => "online_music",
            Self::Audiobook => "audiobook",
            Self::Podcast => "podcast",
            Self::Radio => "radio",
            Self::Ambient => "ambient",
            Self::Recording => "recording",
        }
    }
    pub fn long_form(self) -> bool {
        matches!(self, Self::Audiobook | Self::Podcast)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Media {
    pub id: String,
    pub kind: MediaKind,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub location: String,
    pub duration_ms: u64,
    pub artwork: Option<String>,
    pub favorite: bool,
    /// Remote catalog ID and local identity remain distinct from display metadata.
    pub provider: String,
    pub group_id: String,
    pub track_number: u32,
}

impl Media {
    pub fn new(kind: MediaKind, provider: &str, location: &str, title: &str) -> Self {
        Self {
            id: stable_id(&format!("{}:{}", provider, kind.as_str()), location),
            kind,
            title: title.to_owned(),
            artist: String::new(),
            album: String::new(),
            location: location.to_owned(),
            duration_ms: 0,
            artwork: None,
            favorite: false,
            provider: provider.to_owned(),
            group_id: String::new(),
            track_number: 0,
        }
    }
    pub fn is_remote(&self) -> bool {
        self.location.starts_with("https://")
            || self.location.starts_with("http://")
            || self.location.starts_with("auvemir://")
    }
    pub fn seekable(&self) -> bool {
        self.kind != MediaKind::Radio && !self.is_remote()
    }
}

pub fn stable_id(provider: &str, source: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(provider.as_bytes());
    h.update(&[0]);
    h.update(source.as_bytes());
    h.finalize().to_hex().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Station {
    pub id: String,
    pub name: String,
    pub url: String,
    pub country: String,
    pub language: String,
    pub tags: String,
    pub logo: Option<String>,
    pub custom: bool,
    pub favorite: bool,
}
impl Station {
    pub fn media(&self) -> Media {
        let mut m = Media::new(
            MediaKind::Radio,
            if self.custom {
                "custom-radio"
            } else {
                "radio-browser"
            },
            &self.url,
            &self.name,
        );
        m.id = self.id.clone();
        m.artist = [self.country.as_str(), self.tags.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        m.artwork = self.logo.clone();
        m.favorite = self.favorite;
        m
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bookmark {
    pub id: i64,
    pub media_id: String,
    pub position_ms: u64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogBook {
    pub id: String,
    pub title: String,
    pub author: String,
    pub language: String,
    pub feed_url: String,
    pub cover: Option<String>,
}

pub fn format_time(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
