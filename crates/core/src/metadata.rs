//! User-triggered metadata and playlist-reference integrations. No commercial audio is fetched.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Candidate {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub recording_id: String,
    pub release_id: String,
    pub artist_id: String,
    pub url: String,
    pub artwork: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceSong {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub provider_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferencePlaylist {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub tracks: Vec<ReferenceSong>,
}
#[derive(Clone)]
pub struct Services {
    client: reqwest::Client,
    musicbrainz: Arc<tokio::sync::Mutex<tokio::time::Instant>>,
}
impl Services {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: crate::network::client(false)?,
            musicbrainz: Arc::new(tokio::sync::Mutex::new(tokio::time::Instant::now())),
        })
    }
    async fn json(request: reqwest::RequestBuilder) -> Result<Value> {
        tokio::time::timeout(Duration::from_secs(30), async {
            let mut response = request
                .send()
                .await
                .map_err(|e| e.without_url())?
                .error_for_status()
                .map_err(|e| e.without_url())?;
            ensure!(
                response
                    .content_length()
                    .is_none_or(|n| n <= 4 * 1024 * 1024),
                "Metadata response exceeds 4 MiB"
            );
            let mut data = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|e| e.without_url())? {
                ensure!(
                    data.len() + chunk.len() <= 4 * 1024 * 1024,
                    "Metadata response exceeds 4 MiB"
                );
                data.extend_from_slice(&chunk);
            }
            Ok::<_, anyhow::Error>(serde_json::from_slice(&data)?)
        })
        .await?
    }
    async fn mb(&self, url: &str, query: &[(&str, &str)]) -> Result<Value> {
        let mut gate = self.musicbrainz.lock().await;
        tokio::time::sleep_until(*gate).await;
        *gate = tokio::time::Instant::now() + Duration::from_secs(1);
        drop(gate);
        Self::json(self.client.get(url).query(query)).await
    }
    pub async fn search(&self, title: &str, artist: &str) -> Result<Vec<Candidate>> {
        let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let query = if artist.trim().is_empty() {
            format!("recording:\"{}\"", quote(title))
        } else {
            format!(
                "recording:\"{}\" AND artist:\"{}\"",
                quote(title),
                quote(artist)
            )
        };
        let value = self
            .mb(
                "https://musicbrainz.org/ws/2/recording/",
                &[("query", &query), ("fmt", "json"), ("limit", "25")],
            )
            .await?;
        Ok(value["recordings"]
            .as_array()
            .into_iter()
            .flatten()
            .map(recording)
            .collect())
    }
    pub async fn recording(&self, id: &str) -> Result<Candidate> {
        ensure!(mbid(id), "Invalid MusicBrainz recording ID");
        let value = self
            .mb(
                &format!("https://musicbrainz.org/ws/2/recording/{id}"),
                &[("inc", "artists+releases"), ("fmt", "json")],
            )
            .await?;
        Ok(recording(&value))
    }
    pub async fn recognize(
        &self,
        key: &str,
        duration: u64,
        fingerprint: &str,
    ) -> Result<Vec<Candidate>> {
        let value = Self::json(
            self.client
                .post("https://api.acoustid.org/v2/lookup")
                .form(&[
                    ("client", key.to_owned()),
                    ("duration", duration.to_string()),
                    ("fingerprint", fingerprint.to_owned()),
                    ("meta", "recordings releasegroups".into()),
                    ("format", "json".into()),
                ]),
        )
        .await?;
        ensure!(
            value["status"] == "ok",
            "AcoustID rejected the fingerprint or client key"
        );
        let mut candidates = Vec::new();
        for result in value["results"].as_array().into_iter().flatten() {
            for r in result["recordings"].as_array().into_iter().flatten() {
                let artist = r["artists"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|a| a["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                candidates.push(Candidate {
                    title: text(r, "title"),
                    artist,
                    recording_id: text(r, "id"),
                    ..Default::default()
                });
                if candidates.len() >= 25 {
                    return Ok(candidates);
                }
            }
        }
        Ok(candidates)
    }
    pub async fn genius(&self, token: &str, title: &str, artist: &str) -> Result<Vec<Candidate>> {
        let value = Self::json(
            self.client
                .get("https://api.genius.com/search")
                .bearer_auth(token)
                .query(&[("q", format!("{title} {artist}"))]),
        )
        .await?;
        Ok(value
            .pointer("/response/hits")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|h| {
                let r = &h["result"];
                (!r["title"].is_null()).then(|| Candidate {
                    title: text(r, "title"),
                    artist: text(&r["primary_artist"], "name"),
                    url: text(r, "url"),
                    artwork: text(r, "song_art_image_url"),
                    ..Default::default()
                })
            })
            .take(25)
            .collect())
    }
    pub fn cover(release_id: &str) -> Result<String> {
        ensure!(
            mbid(release_id),
            "Choose a release with a valid MusicBrainz ID"
        );
        Ok(format!(
            "https://coverartarchive.org/release/{release_id}/front-500"
        ))
    }
    pub async fn fanart(&self, key: &str, artist_id: &str) -> Result<String> {
        ensure!(
            mbid(artist_id),
            "Choose an artist with a valid MusicBrainz ID"
        );
        let value = Self::json(
            self.client
                .get(format!("https://webservice.fanart.tv/v3/music/{artist_id}"))
                .query(&[("api_key", key)]),
        )
        .await?;
        let art = value["artistthumb"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a["url"].as_str())
            .context("No artist artwork was returned")?;
        crate::network::validate_url(art, false)?;
        Ok(art.into())
    }
    pub async fn youtube(&self, input: &str, key: &str) -> Result<ReferencePlaylist> {
        ensure!(
            !key.trim().is_empty() && key.len() <= 1024,
            "Add a YouTube Data API key in Settings"
        );
        let id = playlist_id("youtube", input)?;
        let value = Self::json(
            self.client
                .get("https://www.googleapis.com/youtube/v3/playlists")
                .query(&[("part", "snippet"), ("id", &id), ("key", key)]),
        )
        .await?;
        let name = value
            .pointer("/items/0/snippet/title")
            .and_then(Value::as_str)
            .context("Playlist is unavailable or private")?
            .to_owned();
        let mut result = ReferencePlaylist {
            id: id.clone(),
            provider: "youtube".into(),
            name,
            tracks: Vec::new(),
        };
        let mut token = String::new();
        let mut visited = std::collections::HashSet::new();
        for _ in 0..100 {
            let value = Self::json(
                self.client
                    .get("https://www.googleapis.com/youtube/v3/playlistItems")
                    .query(&[
                        ("part", "snippet"),
                        ("playlistId", &id),
                        ("maxResults", "50"),
                        ("pageToken", &token),
                        ("key", key),
                    ]),
            )
            .await?;
            for item in value["items"].as_array().into_iter().flatten() {
                if let Some(track) = youtube_track(item) {
                    ensure!(result.tracks.len() < 5000, "Playlist exceeds 5,000 tracks");
                    result.tracks.push(track);
                }
            }
            token = value["nextPageToken"].as_str().unwrap_or("").to_owned();
            if token.is_empty() {
                return Ok(result);
            }
            ensure!(
                token.len() <= 2048 && visited.insert(token.clone()),
                "Invalid playlist pagination"
            );
        }
        bail!("Playlist exceeds 100 pages")
    }
    pub async fn spotify(&self, input: &str, token: &str) -> Result<ReferencePlaylist> {
        let id = playlist_id("spotify", input)?;
        let metadata = Self::json(
            self.client
                .get(format!("https://api.spotify.com/v1/playlists/{id}"))
                .bearer_auth(token)
                .query(&[("fields", "name")]),
        )
        .await?;
        let mut result = ReferencePlaylist {
            id: id.clone(),
            provider: "spotify".into(),
            name: metadata["name"]
                .as_str()
                .unwrap_or("Spotify playlist")
                .into(),
            tracks: Vec::new(),
        };
        let mut next = Some(format!(
            "https://api.spotify.com/v1/playlists/{id}/items?limit=50"
        ));
        for _ in 0..100 {
            let Some(url) = next.take() else {
                return Ok(result);
            };
            pagination(&url, "api.spotify.com")?;
            let value = Self::json(self.client.get(url).bearer_auth(token)).await?;
            for entry in value["items"].as_array().into_iter().flatten() {
                let track = entry
                    .get("item")
                    .or_else(|| entry.get("track"))
                    .unwrap_or(&Value::Null);
                if track["type"].as_str().is_some_and(|s| s != "track")
                    || track["name"].as_str().is_none()
                {
                    continue;
                }
                result.tracks.push(ReferenceSong {
                    title: text(track, "name"),
                    artist: track["artists"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|a| a["name"].as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    album: text(&track["album"], "name"),
                    provider_id: text(track, "id"),
                });
            }
            next = value["next"].as_str().map(str::to_owned);
        }
        ensure!(next.is_none(), "Playlist exceeds 5,000 tracks");
        Ok(result)
    }
    pub async fn deezer(&self, input: &str) -> Result<ReferencePlaylist> {
        let id = playlist_id("deezer", input)?;
        let value = Self::json(
            self.client
                .get(format!("https://api.deezer.com/playlist/{id}")),
        )
        .await?;
        ensure!(value["error"].is_null(), "Deezer playlist is unavailable");
        let mut result = ReferencePlaylist {
            id,
            provider: "deezer".into(),
            name: value["title"].as_str().unwrap_or("Deezer playlist").into(),
            tracks: Vec::new(),
        };
        let mut page = value["tracks"].clone();
        for _ in 0..100 {
            for t in page["data"].as_array().into_iter().flatten() {
                result.tracks.push(ReferenceSong {
                    title: text(t, "title"),
                    artist: text(&t["artist"], "name"),
                    album: text(&t["album"], "title"),
                    provider_id: t["id"].as_u64().map(|n| n.to_string()).unwrap_or_default(),
                });
            }
            ensure!(result.tracks.len() <= 5000, "Playlist exceeds 5,000 tracks");
            let Some(next) = page["next"].as_str() else {
                return Ok(result);
            };
            pagination(next, "api.deezer.com")?;
            page = Self::json(self.client.get(next)).await?;
        }
        bail!("Playlist pagination exceeds limit")
    }
}
fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").to_owned()
}
pub fn mbid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
fn recording(value: &Value) -> Candidate {
    let artist = value["artist-credit"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| {
            format!(
                "{}{}",
                a["name"]
                    .as_str()
                    .or_else(|| a["artist"]["name"].as_str())
                    .unwrap_or(""),
                a["joinphrase"].as_str().unwrap_or("")
            )
        })
        .collect::<String>();
    let release = value["releases"].as_array().and_then(|a| a.first());
    Candidate {
        title: text(value, "title"),
        artist,
        album: release.map(|r| text(r, "title")).unwrap_or_default(),
        recording_id: text(value, "id"),
        release_id: release.map(|r| text(r, "id")).unwrap_or_default(),
        artist_id: value
            .pointer("/artist-credit/0/artist/id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        ..Default::default()
    }
}
fn pagination(value: &str, host: &str) -> Result<()> {
    let url = crate::network::validate_url(value, false)?;
    ensure!(
        url.scheme() == "https"
            && url.host_str() == Some(host)
            && url.port_or_known_default() == Some(443),
        "Unexpected playlist pagination host"
    );
    Ok(())
}
pub fn playlist_id(provider: &str, input: &str) -> Result<String> {
    let raw = input.trim();
    let id = if provider == "youtube" && raw.starts_with("http") {
        let url = crate::network::validate_url(raw, false)?;
        ensure!(
            url.scheme() == "https"
                && url.port_or_known_default() == Some(443)
                && matches!(
                    url.host_str(),
                    Some("music.youtube.com" | "www.youtube.com" | "youtube.com")
                )
                && matches!(url.path(), "/playlist" | "/watch"),
            "Use an official YouTube playlist link"
        );
        url.query_pairs()
            .find(|(key, _)| key == "list")
            .context("Playlist ID is missing")?
            .1
            .into_owned()
    } else if provider == "spotify" && raw.starts_with("spotify:playlist:") {
        raw.trim_start_matches("spotify:playlist:").to_owned()
    } else if raw.starts_with("http") {
        let url = crate::network::validate_url(raw, false)?;
        let host = if provider == "spotify" {
            "open.spotify.com"
        } else {
            "www.deezer.com"
        };
        ensure!(
            url.host_str() == Some(host)
                || provider == "deezer" && url.host_str() == Some("deezer.com"),
            "Use an official playlist link"
        );
        let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
        let i = parts
            .iter()
            .position(|s| *s == "playlist")
            .context("Link does not identify a playlist")?;
        parts
            .get(i + 1)
            .context("Playlist ID is missing")?
            .to_string()
    } else {
        raw.to_owned()
    };
    ensure!(
        match provider {
            "spotify" => id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric()),
            "deezer" => !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit()),
            "youtube" =>
                !id.is_empty()
                    && id.len() <= 150
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
            _ => false,
        },
        "Invalid playlist provider or ID"
    );
    Ok(id)
}
fn youtube_track(item: &Value) -> Option<ReferenceSong> {
    let snippet = &item["snippet"];
    let id = snippet.pointer("/resourceId/videoId")?.as_str()?;
    let title = snippet["title"].as_str()?.trim();
    if id.len() != 11
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || title.is_empty()
        || matches!(title, "Private video" | "Deleted video")
    {
        return None;
    }
    let owner = snippet["videoOwnerChannelTitle"]
        .as_str()
        .unwrap_or("")
        .trim();
    Some(ReferenceSong {
        title: title.into(),
        artist: owner.strip_suffix(" - Topic").unwrap_or(owner).into(),
        album: String::new(),
        provider_id: id.into(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn youtube_music_links_and_track_metadata() {
        assert_eq!(
            playlist_id(
                "youtube",
                "https://music.youtube.com/playlist?list=PL_test-123"
            )
            .unwrap(),
            "PL_test-123"
        );
        for url in [
            "http://youtube.com/playlist?list=PL1",
            "https://user:secret@youtube.com/playlist?list=PL1",
            "https://youtube.com.evil.example/playlist?list=PL1",
            "https://youtube.com:8443/playlist?list=PL1",
            "https://youtube.com/playlist",
        ] {
            assert!(playlist_id("youtube", url).is_err(), "{url}");
        }
        let mut item = serde_json::json!({"snippet":{"title":" Song ", "resourceId":{"videoId":"abcdefghijk"}, "channelTitle":"Playlist owner", "videoOwnerChannelTitle":"Artist - Topic"}});
        let track = youtube_track(&item).unwrap();
        assert_eq!(track.title, "Song");
        assert_eq!(track.artist, "Artist");
        item["snippet"]["title"] = "Private video".into();
        assert!(youtube_track(&item).is_none());
    }
    #[test]
    fn parses_artist_credits_and_release() {
        let r = recording(
            &serde_json::json!({"id":"123","title":"Song","artist-credit":[{"name":"A","joinphrase":" & ","artist":{"id":"a"}},{"name":"B"}],"releases":[{"id":"r","title":"Album"}]}),
        );
        assert_eq!(r.artist, "A & B");
        assert_eq!(r.album, "Album");
        assert!(mbid("12345678-1234-1234-1234-123456789abc"));
        assert!(!mbid("../../private"));
    }
    #[test]
    fn refuses_credential_pagination_exfiltration() {
        assert!(
            pagination(
                "https://api.spotify.com/v1/playlists/123/items",
                "api.spotify.com"
            )
            .is_ok()
        );
        for url in [
            "https://example.com/items",
            "http://api.spotify.com/items",
            "https://api.spotify.com.evil.example/items",
            "https://user:secret@api.spotify.com/items",
        ] {
            assert!(pagination(url, "api.spotify.com").is_err());
        }
        assert_eq!(
            playlist_id("deezer", "https://www.deezer.com/en/playlist/1234").unwrap(),
            "1234"
        );
        assert!(
            playlist_id(
                "spotify",
                "https://example.com/playlist/1234567890123456789012"
            )
            .is_err()
        );
    }
}

#[derive(Debug)]
pub struct ImportReport {
    pub playlist_id: i64,
    pub matched: usize,
    pub total: usize,
}
fn matching_key(title: &str, artist: &str) -> (String, String) {
    (title.trim().to_lowercase(), artist.trim().to_lowercase())
}
fn library_index(
    db: &crate::database::Store,
) -> Result<std::collections::HashMap<(String, String), String>> {
    let mut q=db.conn.prepare("SELECT id,title,artist FROM media WHERE kind IN ('local_music','online_music') ORDER BY CASE WHEN provider='local' THEN 0 ELSE 1 END,id")?;
    let mut index = std::collections::HashMap::new();
    for row in q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (id, title, artist) = row?;
        index.entry(matching_key(&title, &artist)).or_insert(id);
    }
    Ok(index)
}
fn matches(
    index: &std::collections::HashMap<(String, String), String>,
    tracks: &[ReferenceSong],
) -> Vec<String> {
    tracks
        .iter()
        .filter_map(|t| index.get(&matching_key(&t.title, &t.artist)).cloned())
        .collect()
}
fn replace_entries(tx: &rusqlite::Transaction<'_>, playlist: i64, ids: &[String]) -> Result<()> {
    let previous: Vec<String> = tx
        .prepare_cached(
            "SELECT media_id FROM playlist_entries WHERE playlist_id=?1 ORDER BY position",
        )?
        .query_map([playlist], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if previous == ids {
        return Ok(());
    }
    tx.execute(
        "DELETE FROM playlist_entries WHERE playlist_id=?1",
        [playlist],
    )?;
    let mut insert = tx.prepare_cached(
        "INSERT INTO playlist_entries(playlist_id,position,media_id) VALUES(?1,?2,?3)",
    )?;
    for (i, id) in ids.iter().enumerate() {
        insert.execute(rusqlite::params![playlist, i as i64, id])?;
    }
    Ok(())
}
impl crate::database::Store {
    /// Unavailable tracks stay attached to the imported playlist and resolve when its
    /// exact title/artist match is later imported. Local files take priority.
    pub fn import_reference(&mut self, playlist: &ReferencePlaylist) -> Result<ImportReport> {
        use rusqlite::{OptionalExtension, params};
        ensure!(
            !playlist.name.trim().is_empty()
                && playlist.name.len() <= 8192
                && playlist.tracks.len() <= 5000,
            "Invalid reference playlist"
        );
        ensure!(
            ["spotify", "deezer", "youtube"].contains(&playlist.provider.as_str()),
            "Unknown playlist provider"
        );
        playlist_id(&playlist.provider, &playlist.id)?;
        ensure!(
            playlist
                .tracks
                .iter()
                .all(|t| [&t.title, &t.artist, &t.album, &t.provider_id]
                    .iter()
                    .all(|s| s.len() <= 8192)),
            "Playlist metadata exceeds limits"
        );
        let data = serde_json::to_string(&playlist.tracks)?;
        ensure!(
            data.len() <= 4 * 1024 * 1024,
            "Playlist metadata exceeds 4 MiB"
        );
        let key = format!("{}:{}", playlist.provider, playlist.id);
        let index = library_index(self)?;
        let ids = matches(&index, &playlist.tracks);
        let tx = self.conn.transaction()?;
        let id = if let Some(id) = tx
            .query_row(
                "SELECT playlist_id FROM reference_playlists WHERE id=?1",
                [&key],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
        {
            id
        } else {
            tx.execute("INSERT INTO playlists(name) VALUES(?1)", [&playlist.name])?;
            tx.last_insert_rowid()
        };
        tx.execute("INSERT INTO reference_playlists(id,name,provider,tracks,playlist_id) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name,tracks=excluded.tracks",params![key,playlist.name,playlist.provider,data,id])?;
        replace_entries(&tx, id, &ids)?;
        tx.commit()?;
        Ok(ImportReport {
            playlist_id: id,
            matched: ids.len(),
            total: playlist.tracks.len(),
        })
    }
    pub fn reconcile_references(&mut self) -> Result<usize> {
        let lists: Vec<(i64, String)> = self
            .conn
            .prepare("SELECT playlist_id,tracks FROM reference_playlists")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        if lists.is_empty() {
            return Ok(0);
        }
        let index = library_index(self)?;
        let tx = self.conn.transaction()?;
        let mut total = 0;
        for (id, tracks) in lists {
            let tracks: Vec<ReferenceSong> = serde_json::from_str(&tracks)?;
            let ids = matches(&index, &tracks);
            total += ids.len();
            replace_entries(&tx, id, &ids)?;
        }
        tx.commit()?;
        Ok(total)
    }
    pub fn reference_summaries(&self) -> Result<Vec<(String, String, usize, usize)>> {
        Ok(self.conn.prepare("SELECT r.id,r.name,COUNT(p.media_id),json_array_length(r.tracks) FROM reference_playlists r LEFT JOIN playlist_entries p ON p.playlist_id=r.playlist_id GROUP BY r.id ORDER BY r.name COLLATE NOCASE")?.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get::<_,i64>(2)?.max(0)as usize,r.get::<_,i64>(3)?.max(0)as usize)))?.collect::<rusqlite::Result<_>>()?)
    }
    pub fn reference_missing(&self, id: &str) -> Result<Vec<ReferenceSong>> {
        let tracks: String = self.conn.query_row(
            "SELECT tracks FROM reference_playlists WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        let tracks: Vec<ReferenceSong> = serde_json::from_str(&tracks)?;
        let index = library_index(self)?;
        Ok(tracks
            .into_iter()
            .filter(|t| !index.contains_key(&matching_key(&t.title, &t.artist)))
            .collect())
    }
}
