//! Opt-in scrobbling. Credentials are supplied by the platform adapter, never SQLite.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Listen {
    pub media_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub started_at: i64,
    pub listened_ms: u64,
}
impl Listen {
    pub fn eligible(&self) -> bool {
        self.duration_ms >= 30_000
            && !self.artist.trim().is_empty()
            && self.listened_ms >= (self.duration_ms / 2).min(240_000)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    /// listenbrainz (also Koito / compatible Multi-Scrobbler), lastfm, or maloja.
    pub protocol: String,
    pub endpoint: String,
    pub enabled: bool,
}
impl Service {
    pub fn validate(&self) -> Result<()> {
        let url = crate::network::validate_url(&self.endpoint, true)?;
        if url.query().is_some() || url.fragment().is_some() {
            bail!("Use an endpoint without query parameters or fragments");
        }
        if !["listenbrainz", "lastfm", "librefm", "maloja"].contains(&self.protocol.as_str()) {
            bail!("Choose a supported scrobbling protocol");
        }
        Ok(())
    }
}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub async fn submit(service: &Service, secret: &str, listen: &Listen) -> Result<()> {
    submit_kind(service, secret, listen, false).await
}
pub async fn now_playing(service: &Service, secret: &str, listen: &Listen) -> Result<()> {
    if service.protocol == "maloja" {
        return Ok(());
    }
    submit_kind(service, secret, listen, true).await
}
async fn submit_kind(
    service: &Service,
    secret: &str,
    listen: &Listen,
    playing: bool,
) -> Result<()> {
    service.validate()?;
    if !service.enabled {
        bail!("Scrobbling is disabled");
    }
    let endpoint = crate::network::validate_url(&service.endpoint, true)?;
    // No redirects: a server cannot forward a token to another host.
    let client = reqwest::Client::builder()
        .user_agent("Auvemir/0.1")
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut payload = serde_json::json!({"track_metadata":{"artist_name":listen.artist,"track_name":listen.title,"release_name":listen.album,"additional_info":{"duration_ms":listen.duration_ms,"media_player":"Auvemir","submission_client":"Auvemir","submission_client_version":"0.1.0"}}});
    if !playing {
        payload["listened_at"] = listen.started_at.into();
    }
    let request=match service.protocol.as_str() {
        "listenbrainz" => client.post(url::Url::parse(&format!("{}/",service.endpoint.trim_end_matches('/')))?.join("1/submit-listens")?)
            .header("Authorization",format!("Token {secret}"))
            .json(&serde_json::json!({"listen_type":if playing{"playing_now"}else{"single"},"payload":[payload]})),
        "lastfm"|"librefm" => {
            // API key, application secret, authenticated session key; JSON stays in OS keyring.
            let credentials:serde_json::Value=serde_json::from_str(secret)?;
            let get=|key:&str| credentials[key].as_str().filter(|s|!s.is_empty()).ok_or_else(||anyhow::anyhow!("Last.fm credentials need api_key, api_secret, and session_key"));
            let mut params=BTreeMap::from([("method".to_owned(),"track.scrobble".to_owned()),("api_key".into(),get("api_key")?.into()),("sk".into(),get("session_key")?.into()),("artist".into(),listen.artist.clone()),("track".into(),listen.title.clone()),("album".into(),listen.album.clone()),("timestamp".into(),listen.started_at.to_string()),("duration".into(),(listen.duration_ms/1000).to_string())]);
            if playing {params.insert("method".into(),"track.updateNowPlaying".into());params.remove("timestamp");}
            let signature=sign(&params,get("api_secret")?);
            params.insert("api_sig".into(),signature); params.insert("format".into(),"json".into());
            client.post(endpoint).form(&params)
        }
        "maloja" => client.post(url::Url::parse(&format!("{}/",service.endpoint.trim_end_matches('/')))?.join("apis/mlj_1/newscrobble")?).json(&serde_json::json!({"key":secret,"artists":[listen.artist],"title":listen.title,"album":listen.album,"time":listen.started_at,"length":listen.duration_ms/1000})),
        _ => bail!("Unknown scrobbling protocol"),
    };
    let mut response = request
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Scrobbling server could not be reached"))?;
    if !response.status().is_success() {
        bail!(
            "Scrobbling server returned HTTP {}",
            response.status().as_u16()
        );
    }
    let mut data = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if data.len() + chunk.len() > 256 * 1024 {
            bail!("Scrobbling response exceeds limit");
        }
        data.extend(chunk);
    }
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data)
        && (value["status"] == "error" || value.get("error").is_some_and(|e| !e.is_null()))
    {
        bail!("Scrobbling server rejected the listen");
    }
    if matches!(service.protocol.as_str(), "lastfm" | "librefm") {
        let result: serde_json::Value = serde_json::from_slice(&data)?;
        if result.get("error").is_some()
            || result
                .pointer("/scrobbles/@attr/accepted")
                .and_then(|v| v.as_str())
                .is_some_and(|v| v == "0")
            || result
                .pointer("/scrobbles/@attr/accepted")
                .and_then(|v| v.as_u64())
                == Some(0)
            || result
                .pointer("/nowplaying/ignoredMessage/code")
                .is_some_and(|v| {
                    v.as_str().is_some_and(|s| s != "0") || v.as_u64().is_some_and(|n| n != 0)
                })
        {
            bail!("Last.fm did not accept the listen; check the session and metadata");
        }
    }
    Ok(())
}
pub fn sign(params: &BTreeMap<String, String>, secret: &str) -> String {
    let mut text = String::new();
    for (key, value) in params {
        if !["format", "callback", "api_sig"].contains(&key.as_str()) {
            text.push_str(key);
            text.push_str(value);
        }
    }
    text.push_str(secret);
    format!("{:x}", md5::compute(text.as_bytes()))
}
