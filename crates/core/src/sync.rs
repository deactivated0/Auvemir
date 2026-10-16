//! Portable, encrypted metadata sync. Audio, paths and credentials never enter a snapshot.
use crate::{database::Store, model::Media};
use anyhow::{Context, Result, bail, ensure};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::RngCore;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

const MAGIC: &[u8; 8] = b"AUVSYNC1";
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECORDS: usize = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Value {
    Favorite(bool),
    Progress(u64),
    Setting(String),
    Playlist(Option<Playlist>),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Playlist {
    pub name: String,
    pub items: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub counter: u64,
    pub device: String,
    pub value: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub version: u32,
    pub records: BTreeMap<String, Record>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            version: 1,
            records: BTreeMap::new(),
        }
    }
}
pub fn allowed_setting(key: &str) -> bool {
    matches!(key, "volume" | "speed" | "equalizer")
}
fn hex_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
fn identity_id(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn identity(m: &Media) -> String {
    if m.provider != "local" {
        return crate::model::stable_id(&m.provider, &m.id);
    }
    let normalized = format!(
        "{}\0{}\0{}\0{}\0{}\0{}",
        m.kind.as_str(),
        m.title.trim().to_lowercase(),
        m.artist.trim().to_lowercase(),
        m.album.trim().to_lowercase(),
        m.duration_ms / 1000,
        m.track_number
    );
    crate::model::stable_id("portable-media-v1", &normalized)
}
fn device_id() -> String {
    let mut b = [0u8; 16];
    rand::rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}
impl Snapshot {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported sync format");
        ensure!(self.records.len() <= MAX_RECORDS, "Too many sync records");
        for (key, r) in &self.records {
            ensure!(
                hex_id(&r.device) && r.counter > 0 && r.counter < i64::MAX as u64,
                "Invalid sync revision"
            );
            match (&r.value, key.split_once(':')) {
                (Value::Favorite(_), Some(("f", id))) | (Value::Progress(_), Some(("p", id))) => {
                    ensure!(identity_id(id), "Invalid media identity")
                }
                (Value::Setting(v), Some(("s", k))) => {
                    ensure!(
                        allowed_setting(k) && v.len() <= 2048,
                        "Setting is not syncable"
                    );
                    match k {
                        "volume" => ensure!(
                            v.parse::<f32>()
                                .is_ok_and(|x| x.is_finite() && (0.0..=1.0).contains(&x)),
                            "Invalid volume"
                        ),
                        "speed" => ensure!(
                            v.parse::<f32>()
                                .is_ok_and(|x| x.is_finite() && (0.5..=2.0).contains(&x)),
                            "Invalid speed"
                        ),
                        _ => {
                            let g: Vec<f32> = serde_json::from_str(v)?;
                            ensure!(
                                g.len() == 10
                                    && g.iter()
                                        .all(|x| x.is_finite() && (-12.0..=12.0).contains(x)),
                                "Invalid equalizer"
                            );
                        }
                    }
                }
                (Value::Playlist(p), Some(("l", id))) => {
                    ensure!(hex_id(id), "Invalid playlist identity");
                    if let Some(p) = p {
                        ensure!(
                            !p.name.trim().is_empty()
                                && p.name.len() <= 8192
                                && p.items.len() <= MAX_RECORDS
                                && p.items.iter().all(|s| identity_id(s)),
                            "Invalid playlist"
                        );
                    }
                }
                _ => bail!("Invalid sync key or value"),
            }
        }
        Ok(())
    }
    pub fn merge(&mut self, other: &Self) -> Result<()> {
        self.validate()?;
        other.validate()?;
        for (k, v) in &other.records {
            match self.records.get(k) {
                Some(old) if (old.counter, &old.device) == (v.counter, &v.device) => {
                    ensure!(old == v, "Conflicting sync revision")
                }
                Some(old) if (old.counter, &old.device) > (v.counter, &v.device) => {}
                _ => {
                    self.records.insert(k.clone(), v.clone());
                }
            }
        }
        ensure!(
            self.records.len() <= MAX_RECORDS,
            "Too many merged sync records"
        );
        Ok(())
    }
}
fn cipher(pass: &str, salt: &[u8]) -> Result<XChaCha20Poly1305> {
    ensure!(
        pass.chars().count() >= 12 && pass.len() <= 4096,
        "Use a sync passphrase of at least 12 characters"
    );
    let mut key = [0u8; 32];
    let params = Params::new(64 * 1024, 3, 1, Some(32)).map_err(|e| anyhow::anyhow!("{e}"))?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(pass.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key)?;
    // The KDF's temporary key is no longer needed after cipher construction.
    key.fill(0);
    Ok(cipher)
}
pub fn encrypt(snapshot: &Snapshot, pass: &str) -> Result<Vec<u8>> {
    snapshot.validate()?;
    let data = serde_json::to_vec(snapshot)?;
    ensure!(data.len() <= MAX_BYTES - 64, "Sync snapshot is too large");
    let mut salt = [0; 16];
    let mut nonce = [0; 24];
    rand::rng().fill_bytes(&mut salt);
    rand::rng().fill_bytes(&mut nonce);
    let encrypted = cipher(pass, &salt)?
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &data,
                aad: MAGIC,
            },
        )
        .map_err(|_| anyhow::anyhow!("Sync encryption failed"))?;
    let mut out = Vec::with_capacity(48 + encrypted.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&encrypted);
    Ok(out)
}
pub fn decrypt(bytes: &[u8], pass: &str) -> Result<Snapshot> {
    ensure!(
        (64..=MAX_BYTES).contains(&bytes.len()) && &bytes[..8] == MAGIC,
        "Invalid encrypted sync file"
    );
    let data = cipher(pass, &bytes[8..24])?
        .decrypt(
            XNonce::from_slice(&bytes[24..48]),
            Payload {
                msg: &bytes[48..],
                aad: MAGIC,
            },
        )
        .map_err(|_| anyhow::anyhow!("Wrong sync passphrase or damaged file"))?;
    let snapshot: Snapshot = serde_json::from_slice(&data)?;
    snapshot.validate()?;
    Ok(snapshot)
}

impl Store {
    pub(crate) fn sync_put(&self, key: &str, value: &Value) -> Result<()> {
        let device = self
            .setting("sync-device")?
            .context("Sync is not initialized")?;
        let old: Option<String> = self
            .conn
            .prepare_cached("SELECT value FROM sync_records WHERE key=?1")?
            .query_row([key], |r| r.get(0))
            .optional()?;
        let value = serde_json::to_string(value)?;
        if old.as_ref() == Some(&value) {
            return Ok(());
        }
        self.conn.prepare_cached("INSERT INTO sync_records(key,counter,device,value) VALUES(?1,(SELECT COALESCE(MAX(counter),0)+1 FROM sync_records),?2,?3) ON CONFLICT(key) DO UPDATE SET counter=excluded.counter,device=excluded.device,value=excluded.value")?.execute(params![key,device,value])?;
        Ok(())
    }
    pub(crate) fn sync_touch_media(&self, id: &str, progress: bool) -> Result<()> {
        if self.setting("sync-device")?.is_none() {
            return Ok(());
        }
        if let Some(m) = self.get(id)? {
            let key = identity(&m);
            self.sync_put(
                &format!("{}:{key}", if progress { "p" } else { "f" }),
                &if progress {
                    Value::Progress(self.progress(id)?)
                } else {
                    Value::Favorite(m.favorite)
                },
            )?;
        }
        Ok(())
    }
    fn sync_records(&self) -> Result<Snapshot> {
        let mut q = self
            .conn
            .prepare("SELECT key,counter,device,value FROM sync_records")?;
        let mut out = Snapshot::default();
        for row in q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)? as u64,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })? {
            let (k, counter, device, v) = row?;
            out.records.insert(
                k,
                Record {
                    counter,
                    device,
                    value: serde_json::from_str(&v)?,
                },
            );
        }
        Ok(out)
    }
    /// Three bulk reads for media/progress/playlists, avoiding one query per playlist item.
    pub fn sync_snapshot(&mut self) -> Result<Snapshot> {
        if self.setting("sync-device")?.is_none() {
            self.set("sync-device", &device_id())?;
        }
        let snapshot = self.sync_records()?;
        let known: HashSet<String> = self
            .conn
            .prepare("SELECT identity FROM sync_known_media")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut q=self.conn.prepare("SELECT m.id,COALESCE(p.position_ms,0) FROM media m LEFT JOIN progress p ON p.media_id=m.id")?;
        let progress: HashMap<String, u64> = q
            .query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)?.max(0) as u64)))?
            .collect::<rusqlite::Result<_>>()?;
        drop(q);
        let mut ids = HashMap::new();
        // Reuse the existing decoder to keep persistent media semantics unchanged.
        let media = self.all_media()?;
        for m in &media {
            let identity = identity(m);
            ids.insert(m.id.clone(), identity.clone());
            if known.contains(&identity) && snapshot.records.contains_key(&format!("f:{identity}"))
                || !snapshot.records.contains_key(&format!("f:{identity}")) && m.favorite
            {
                self.sync_put(&format!("f:{identity}"), &Value::Favorite(m.favorite))?;
            }
            let ms = progress.get(&m.id).copied().unwrap_or(0);
            if known.contains(&identity) && snapshot.records.contains_key(&format!("p:{identity}"))
                || !snapshot.records.contains_key(&format!("p:{identity}")) && ms > 0
            {
                self.sync_put(&format!("p:{identity}"), &Value::Progress(ms))?;
            }
        }
        drop(media);
        drop(progress);
        let mut lists = self.playlists()?;
        let mut entries: HashMap<i64, Vec<String>> = HashMap::new();
        let mut q = self.conn.prepare(
            "SELECT playlist_id,media_id FROM playlist_entries ORDER BY playlist_id,position",
        )?;
        for row in q.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (id, m) = row?;
            if let Some(portable) = ids.get(&m) {
                entries.entry(id).or_default().push(portable.clone());
            }
        }
        drop(q);
        let mut existing = HashSet::new();
        let available: HashSet<&String> = ids.values().collect();
        for (id, name) in lists.drain(..) {
            let sync_id: Option<String> =
                self.conn
                    .query_row("SELECT sync_id FROM playlists WHERE id=?1", [id], |r| {
                        r.get(0)
                    })?;
            let sync_id = match sync_id {
                Some(s) => s,
                None => {
                    let s = device_id();
                    self.conn.execute(
                        "UPDATE playlists SET sync_id=?2 WHERE id=?1",
                        params![id, s],
                    )?;
                    s
                }
            };
            let key = format!("l:{sync_id}");
            existing.insert(key.clone());
            let mut items = entries.remove(&id).unwrap_or_default();
            if let Some(Record {
                value: Value::Playlist(Some(old)),
                ..
            }) = snapshot.records.get(&key)
            {
                let old_available: Vec<_> = old
                    .items
                    .iter()
                    .filter(|i| available.contains(i) && known.contains(*i))
                    .cloned()
                    .collect();
                if old_available == items {
                    items = old.items.clone();
                }
            }
            self.sync_put(&key, &Value::Playlist(Some(Playlist { name, items })))?;
        }
        for (key, r) in &snapshot.records {
            if let Value::Playlist(Some(_)) = &r.value
                && !existing.contains(key)
                && self.conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sync_known_media WHERE identity=?1)",
                    [key],
                    |r| r.get::<_, bool>(0),
                )?
            {
                self.sync_put(key, &Value::Playlist(None))?;
            }
        }
        for key in ["volume", "speed", "equalizer"] {
            if let Some(v) = self.setting(key)?
                && !snapshot.records.contains_key(&format!("s:{key}"))
            {
                self.sync_put(&format!("s:{key}"), &Value::Setting(v))?;
            }
        }
        drop(available);
        drop(ids);
        drop(known);
        drop(entries);
        drop(existing);
        drop(snapshot);
        let snapshot = self.sync_records()?;
        snapshot.validate()?;
        // First apply also resolves remote records for media imported since the last merge.
        self.sync_apply(&snapshot)?;
        Ok(snapshot)
    }
    pub fn sync_merge(&mut self, other: &Snapshot) -> Result<Snapshot> {
        let mut merged = self.sync_snapshot()?;
        merged.merge(other)?;
        self.sync_apply(&merged)?;
        Ok(merged)
    }
    fn sync_apply(&mut self, snapshot: &Snapshot) -> Result<()> {
        snapshot.validate()?;
        let media = self.all_media()?;
        let mut ids: HashMap<String, Vec<String>> = HashMap::new();
        for m in media {
            ids.entry(identity(&m)).or_default().push(m.id);
        }
        let tx = self.conn.transaction()?;
        for (key, r) in &snapshot.records {
            tx.prepare_cached("INSERT INTO sync_records(key,counter,device,value) VALUES(?1,?2,?3,?4) ON CONFLICT(key) DO UPDATE SET counter=excluded.counter,device=excluded.device,value=excluded.value")?.execute(params![key,r.counter as i64,r.device,serde_json::to_string(&r.value)?])?;
            let id = &key[2..];
            match &r.value {
                Value::Favorite(v) => {
                    if let Some(local) = ids.get(id) {
                        for m in local {
                            tx.prepare_cached("UPDATE media SET favorite=?2 WHERE id=?1")?
                                .execute(params![m, v])?;
                            tx.prepare_cached("UPDATE stations SET favorite=?2 WHERE id=?1")?
                                .execute(params![m, v])?;
                        }
                    }
                }
                Value::Progress(v) => {
                    ensure!(*v <= i64::MAX as u64, "Progress exceeds supported range");
                    if let Some(local) = ids.get(id) {
                        for m in local {
                            tx.prepare_cached("INSERT INTO progress(media_id,position_ms,updated) VALUES(?1,?2,unixepoch()) ON CONFLICT(media_id) DO UPDATE SET position_ms=excluded.position_ms,updated=excluded.updated")?.execute(params![m,*v as i64])?;
                        }
                    }
                }
                Value::Setting(v) => {
                    tx.prepare_cached("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value")?.execute(params![id,v])?;
                }
                Value::Playlist(p) => {
                    let old: Option<i64> = tx
                        .query_row("SELECT id FROM playlists WHERE sync_id=?1", [id], |r| {
                            r.get(0)
                        })
                        .optional()?;
                    match p {
                        None => {
                            tx.prepare_cached("DELETE FROM playlists WHERE sync_id=?1")?
                                .execute([id])?;
                        }
                        Some(p) => {
                            let list = if let Some(old) = old {
                                tx.prepare_cached("UPDATE playlists SET name=?2 WHERE id=?1")?
                                    .execute(params![old, p.name])?;
                                old
                            } else {
                                tx.prepare_cached(
                                    "INSERT INTO playlists(name,sync_id) VALUES(?1,?2)",
                                )?
                                .execute(params![p.name, id])?;
                                tx.last_insert_rowid()
                            };
                            tx.prepare_cached("DELETE FROM playlist_entries WHERE playlist_id=?1")?
                                .execute([list])?;
                            let mut insert=tx.prepare_cached("INSERT INTO playlist_entries(playlist_id,position,media_id) VALUES(?1,?2,?3)")?;
                            let mut position = 0i64;
                            for portable in &p.items {
                                if let Some(local) = ids.get(portable).and_then(|v| v.first()) {
                                    insert.execute(params![list, position, local])?;
                                    position += 1;
                                }
                            }
                        }
                    }
                    tx.prepare_cached(
                        "INSERT OR IGNORE INTO sync_known_media(identity) VALUES(?1)",
                    )?
                    .execute([key])?;
                }
            }
        }
        for id in ids.keys() {
            tx.prepare_cached("INSERT OR IGNORE INTO sync_known_media(identity) VALUES(?1)")?
                .execute([id])?;
        }
        tx.commit()?;
        Ok(())
    }
    fn all_media(&self) -> Result<Vec<Media>> {
        let mut q=self.conn.prepare("SELECT id,kind,title,artist,album,location,duration_ms,artwork,favorite,provider,group_id,track_number FROM media")?;
        Ok(q.query_map([], crate::database::decode_media)?
            .collect::<rusqlite::Result<_>>()?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDav {
    pub endpoint: String,
    pub username: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Secret {
    pub password: String,
    pub passphrase: String,
}
impl WebDav {
    pub async fn exchange(&self, secret: &Secret, local: Snapshot) -> Result<Snapshot> {
        crate::network::validate_url(&self.endpoint, true)?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        for _ in 0..3 {
            let mut response = client
                .get(&self.endpoint)
                .basic_auth(&self.username, Some(&secret.password))
                .send()
                .await?;
            let mut merged = local.clone();
            let etag = if response.status() == reqwest::StatusCode::NOT_FOUND {
                None
            } else {
                response.error_for_status_ref()?;
                let etag = response
                    .headers()
                    .get(reqwest::header::ETAG)
                    .context("Sync server must provide ETag conflict protection")?
                    .to_str()?
                    .to_owned();
                ensure!(
                    response
                        .content_length()
                        .is_none_or(|n| n <= MAX_BYTES as u64),
                    "Sync file exceeds 16 MiB"
                );
                let mut data = Vec::new();
                while let Some(chunk) = response.chunk().await? {
                    ensure!(
                        data.len() + chunk.len() <= MAX_BYTES,
                        "Sync file exceeds 16 MiB"
                    );
                    data.extend_from_slice(&chunk);
                }
                let pass = secret.passphrase.clone();
                let remote = tokio::task::spawn_blocking(move || decrypt(&data, &pass)).await??;
                merged.merge(&remote)?;
                Some(etag)
            };
            let pass = secret.passphrase.clone();
            let out = merged.clone();
            let body = tokio::task::spawn_blocking(move || encrypt(&out, &pass)).await??;
            let mut request = client
                .put(&self.endpoint)
                .basic_auth(&self.username, Some(&secret.password))
                .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                .body(body);
            request = if let Some(etag) = etag {
                request.header(reqwest::header::IF_MATCH, etag)
            } else {
                request.header(reqwest::header::IF_NONE_MATCH, "*")
            };
            let response = request.send().await?;
            if response.status() == reqwest::StatusCode::PRECONDITION_FAILED {
                continue;
            }
            response.error_for_status()?;
            return Ok(merged);
        }
        bail!("Sync changed repeatedly on another device. Try again shortly")
    }
}
