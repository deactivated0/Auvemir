use crate::model::{Bookmark, Media, MediaKind, MediaSort, Station};
use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use std::{collections::HashSet, path::Path, time::Duration};

pub struct Store {
    pub(crate) conn: Connection,
}
const MIGRATION: &str = include_str!("schema.sql");

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;",
        )?;
        let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 5 {
            bail!("Database was created by a newer Auvemir version");
        }
        if version == 0 {
            conn.execute_batch(MIGRATION)?;
        }
        if version < 2 {
            conn.execute_batch(include_str!("migration2.sql"))?;
        }
        if version < 3 {
            conn.execute_batch(include_str!("migration3.sql"))?;
        }
        if version < 4 {
            conn.execute_batch(include_str!("migration4.sql"))?;
        }
        if version < 5 {
            conn.execute_batch(include_str!("migration5.sql"))?;
        }
        // Keep Lamport revision allocation logarithmic as the sync log grows.
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS sync_records_counter ON sync_records(counter)",
        )?;
        Ok(Self { conn })
    }
    pub fn memory() -> Result<Self> {
        Self::open(":memory:")
    }
    pub fn upsert(&mut self, items: &[Media]) -> Result<()> {
        let tx = self.conn.transaction()?;
        Self::write_media(&tx, items)?;
        tx.commit()?;
        Ok(())
    }
    fn write_media(conn: &Connection, items: &[Media]) -> Result<()> {
        let mut q = conn.prepare_cached("INSERT INTO media(id,kind,title,artist,album,location,duration_ms,artwork,favorite,provider,group_id,track_number) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(id) DO UPDATE SET title=excluded.title,artist=excluded.artist,album=excluded.album,location=excluded.location,duration_ms=excluded.duration_ms,artwork=excluded.artwork,group_id=excluded.group_id,track_number=excluded.track_number")?;
        for m in items {
            q.execute(params![
                m.id,
                m.kind.as_str(),
                m.title,
                m.artist,
                m.album,
                m.location,
                sql_int(m.duration_ms)?,
                m.artwork,
                m.favorite,
                m.provider,
                m.group_id,
                m.track_number
            ])?;
        }
        Ok(())
    }
    /// Metadata and scan fingerprints must commit together so failed batches are retried.
    pub fn save_scan(&mut self, scan: &crate::library::Scanned) -> Result<()> {
        let tx = self.conn.transaction()?;
        Self::write_media(&tx, &scan.items)?;
        {
            let mut q = tx.prepare_cached("INSERT INTO files(path,size,modified) VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET size=excluded.size,modified=excluded.modified")?;
            for (path, size, modified) in &scan.fingerprints {
                q.execute(params![path, sql_int(*size)?, sql_int(*modified)?])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn media(
        &self,
        kind: Option<MediaKind>,
        search: &str,
        offset: usize,
        favorites: bool,
    ) -> Result<Vec<Media>> {
        self.media_sorted(kind, search, offset, favorites, MediaSort::Album)
    }
    pub fn media_sorted(
        &self,
        kind: Option<MediaKind>,
        search: &str,
        offset: usize,
        favorites: bool,
        sort: MediaSort,
    ) -> Result<Vec<Media>> {
        // Escaped LIKE patterns; all user values are parameters.
        let pattern = format!(
            "%{}%",
            search
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let sql = format!(
            "SELECT id,kind,title,artist,album,location,duration_ms,artwork,favorite,provider,group_id,track_number FROM media WHERE (?1 IS NULL OR kind=?1) AND (?2=0 OR favorite=1) AND (title LIKE ?3 ESCAPE '\\' OR artist LIKE ?3 ESCAPE '\\' OR album LIKE ?3 ESCAPE '\\') ORDER BY {} LIMIT 100 OFFSET ?4",
            sort.order_by()
        );
        let mut q = self.conn.prepare_cached(&sql)?;
        Ok(q.query_map(
            params![
                kind.map(|k| k.as_str()),
                favorites,
                pattern,
                sql_int(offset as u64)?
            ],
            decode_media,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn get(&self, id: &str) -> Result<Option<Media>> {
        Ok(self.conn.query_row("SELECT id,kind,title,artist,album,location,duration_ms,artwork,favorite,provider,group_id,track_number FROM media WHERE id=?1", [id], decode_media).optional()?)
    }
    pub fn count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM media", [], |r| r.get::<_, i64>(0))? as u64)
    }
    pub fn favorite(&self, id: &str) -> Result<()> {
        self.conn
            .execute("UPDATE media SET favorite=NOT favorite WHERE id=?1", [id])?;
        self.conn.execute("UPDATE stations SET favorite=(SELECT favorite FROM media WHERE media.id=stations.id) WHERE id=?1", [id])?;
        self.sync_touch_media(id, false)?;
        Ok(())
    }
    pub fn progress(&self, id: &str) -> Result<u64> {
        Ok(self
            .conn
            .query_row(
                "SELECT position_ms FROM progress WHERE media_id=?1",
                [id],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0)
            .max(0) as u64)
    }
    pub fn save_progress(&self, id: &str, ms: u64) -> Result<()> {
        self.conn.execute("INSERT INTO progress(media_id,position_ms,updated) VALUES(?1,?2,unixepoch()) ON CONFLICT(media_id) DO UPDATE SET position_ms=excluded.position_ms,updated=excluded.updated", params![id,sql_int(ms)?])?;
        self.sync_touch_media(id, true)?;
        Ok(())
    }
    pub fn played(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO history(media_id,played_at) VALUES(?1,unixepoch())",
            [id],
        )?;
        Ok(())
    }
    pub fn recent(&self) -> Result<Vec<Media>> {
        let mut q=self.conn.prepare_cached("SELECT m.id,m.kind,m.title,m.artist,m.album,m.location,m.duration_ms,m.artwork,m.favorite,m.provider,m.group_id,m.track_number FROM media m JOIN (SELECT media_id,MAX(played_at) t FROM history GROUP BY media_id) h ON h.media_id=m.id ORDER BY h.t DESC LIMIT 100")?;
        Ok(q.query_map([], decode_media)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn bookmark(&self, id: &str, ms: u64, name: &str) -> Result<()> {
        if name.trim().is_empty() {
            bail!("Give the bookmark a name");
        }
        self.conn.execute(
            "INSERT INTO bookmarks(media_id,position_ms,name) VALUES(?1,?2,?3)",
            params![id, sql_int(ms)?, name.trim()],
        )?;
        Ok(())
    }
    pub fn bookmarks(&self, id: &str) -> Result<Vec<Bookmark>> {
        let mut q=self.conn.prepare_cached("SELECT id,media_id,position_ms,name FROM bookmarks WHERE media_id=?1 ORDER BY position_ms")?;
        Ok(q.query_map([id], |r| {
            Ok(Bookmark {
                id: r.get(0)?,
                media_id: r.get(1)?,
                position_ms: r.get::<_, i64>(2)?.max(0) as u64,
                name: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn remove_bookmark(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM bookmarks WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value])?;
        if crate::sync::allowed_setting(key) && self.setting("sync-device")?.is_some() {
            self.sync_put(
                &format!("s:{key}"),
                &crate::sync::Value::Setting(value.into()),
            )?;
        }
        Ok(())
    }
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .prepare_cached("SELECT value FROM settings WHERE key=?1")?
            .query_row([key], |r| r.get(0))
            .optional()?)
    }
    pub fn save_queue(&self, ids: &[String], cursor: usize) -> Result<()> {
        self.set("queue", &serde_json::to_string(&(ids, cursor))?)
    }
    pub fn load_queue(&self) -> Result<(Vec<String>, usize)> {
        Ok(self
            .setting("queue")?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default())
    }
    pub fn playlist(&mut self, name: &str, ids: &[String]) -> Result<i64> {
        if name.trim().is_empty() {
            bail!("Give the playlist a name");
        }
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO playlists(name) VALUES(?1)", [name.trim()])?;
        let id = tx.last_insert_rowid();
        for (i, item) in ids.iter().enumerate() {
            tx.execute(
                "INSERT INTO playlist_entries(playlist_id,position,media_id) VALUES(?1,?2,?3)",
                params![id, sql_int(i as u64)?, item],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }
    pub fn playlists(&self) -> Result<Vec<(i64, String)>> {
        let mut q = self
            .conn
            .prepare_cached("SELECT id,name FROM playlists ORDER BY name COLLATE NOCASE")?;
        Ok(q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn playlist_items(&self, id: i64) -> Result<Vec<Media>> {
        let mut q=self.conn.prepare_cached("SELECT m.id,m.kind,m.title,m.artist,m.album,m.location,m.duration_ms,m.artwork,m.favorite,m.provider,m.group_id,m.track_number FROM media m JOIN playlist_entries p ON p.media_id=m.id WHERE p.playlist_id=?1 ORDER BY p.position")?;
        Ok(q.query_map([id], decode_media)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn rename_playlist(&self, id: i64, name: &str) -> Result<()> {
        if name.trim().is_empty() {
            bail!("Give the playlist a name");
        }
        self.conn.execute(
            "UPDATE playlists SET name=?2 WHERE id=?1",
            params![id, name.trim()],
        )?;
        Ok(())
    }
    pub fn delete_playlist(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM playlists WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn save_station(&mut self, s: &Station) -> Result<()> {
        crate::network::validate_url(&s.url, true)?;
        if s.name.trim().is_empty() {
            bail!("Give the station a name");
        }
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO stations(id,name,url,country,language,tags,logo,custom,favorite) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET name=excluded.name,url=excluded.url,country=excluded.country,language=excluded.language,tags=excluded.tags,logo=excluded.logo",params![s.id,s.name,s.url,s.country,s.language,s.tags,s.logo,s.custom,s.favorite])?;
        tx.commit()?;
        self.upsert(&[s.media()])
    }
    pub fn stations(&self) -> Result<Vec<Station>> {
        let mut q=self.conn.prepare_cached("SELECT id,name,url,country,language,tags,logo,custom,favorite FROM stations ORDER BY name COLLATE NOCASE")?;
        Ok(q.query_map([], |r| {
            Ok(Station {
                id: r.get(0)?,
                name: r.get(1)?,
                url: r.get(2)?,
                country: r.get(3)?,
                language: r.get(4)?,
                tags: r.get(5)?,
                logo: r.get(6)?,
                custom: r.get(7)?,
                favorite: r.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn delete_station(&mut self, id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        let custom: bool = tx.query_row("SELECT custom FROM stations WHERE id=?1", [id], |r| {
            r.get(0)
        })?;
        if !custom {
            bail!(
                "Only custom stations can be deleted. Remove a public station from favorites instead."
            );
        }
        tx.execute("DELETE FROM stations WHERE id=?1", [id])?;
        tx.execute("DELETE FROM media WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn fingerprints(&self) -> Result<std::collections::HashMap<String, (u64, u64)>> {
        let mut q = self
            .conn
            .prepare_cached("SELECT path,size,modified FROM files")?;
        Ok(q.query_map([], |r| {
            Ok((
                r.get(0)?,
                (
                    r.get::<_, i64>(1)?.max(0) as u64,
                    r.get::<_, i64>(2)?.max(0) as u64,
                ),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn fingerprint(&self, path: &str, size: u64, modified: u64) -> Result<()> {
        self.conn.execute("INSERT INTO files(path,size,modified) VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET size=excluded.size,modified=excluded.modified",params![path,sql_int(size)?,sql_int(modified)?])?;
        Ok(())
    }
    pub fn lyrics(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT text FROM lyrics WHERE media_id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?)
    }
    pub fn save_lyrics(&self, id: &str, text: &str) -> Result<()> {
        self.conn.execute("INSERT INTO lyrics(media_id,text) VALUES(?1,?2) ON CONFLICT(media_id) DO UPDATE SET text=excluded.text",params![id,text])?;
        Ok(())
    }
    pub fn subscription(&self, url: &str, title: &str, kind: MediaKind) -> Result<()> {
        self.conn.execute("INSERT INTO subscriptions(url,title,kind) VALUES(?1,?2,?3) ON CONFLICT(url) DO UPDATE SET title=excluded.title",params![url,title,kind.as_str()])?;
        Ok(())
    }
    pub fn subscriptions(&self) -> Result<Vec<(String, String, MediaKind)>> {
        let mut q = self
            .conn
            .prepare_cached("SELECT url,title,kind FROM subscriptions ORDER BY title")?;
        Ok(q.query_map([], |r| {
            let s: String = r.get(2)?;
            Ok((r.get(0)?, r.get(1)?, parse_kind(&s)))
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn attach_download(&self, id: &str, path: &str) -> Result<()> {
        self.conn.execute("INSERT INTO downloads(media_id,path) VALUES(?1,?2) ON CONFLICT(media_id) DO UPDATE SET path=excluded.path",params![id,path])?;
        Ok(())
    }
    pub fn download_path(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT path FROM downloads WHERE media_id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?)
    }
    /// Download indicators for a visible page, preserving legacy path conversion rules.
    pub fn download_flags(&self, ids: &[&str]) -> Result<HashSet<String>> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }
        let encoded = serde_json::to_string(ids)?;
        let mut q = self.conn.prepare_cached("SELECT media_id,path FROM downloads WHERE media_id IN (SELECT value FROM json_each(?1))")?;
        let rows = q.query_map([encoded], |r| {
            let id: String = r.get(0)?;
            Ok(r.get_ref(1)?.as_str().ok().map(|_| id))
        })?;
        let mut out = HashSet::new();
        for row in rows {
            if let Some(id) = row? {
                out.insert(id);
            }
        }
        Ok(out)
    }
    pub fn groups(&self, kind: MediaKind, field: &str) -> Result<Vec<(String, String, u64)>> {
        let field = match field {
            "artist" => "artist",
            "album" => "album",
            "group_id" => "group_id",
            _ => bail!("Unknown grouping"),
        };
        let mut q = self.conn.prepare(&format!("SELECT {field},CASE WHEN '{field}'='group_id' THEN MAX(album) ELSE {field} END,COUNT(*) FROM media WHERE kind=?1 AND {field}<>'' GROUP BY {field} ORDER BY 2 COLLATE NOCASE LIMIT 500"))?;
        Ok(q.query_map([kind.as_str()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u64))
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn group_items(&self, kind: MediaKind, field: &str, value: &str) -> Result<Vec<Media>> {
        let field = match field {
            "artist" => "artist",
            "album" => "album",
            "group_id" => "group_id",
            _ => bail!("Unknown grouping"),
        };
        let mut q=self.conn.prepare(&format!("SELECT id,kind,title,artist,album,location,duration_ms,artwork,favorite,provider,group_id,track_number FROM media WHERE kind=?1 AND {field}=?2 ORDER BY album COLLATE NOCASE,track_number,title COLLATE NOCASE LIMIT 1000"))?;
        Ok(q.query_map(params![kind.as_str(), value], decode_media)?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn record_listen(
        &self,
        listen: &crate::scrobble::Listen,
        services: &[String],
    ) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        let media_id = self.get(&listen.media_id)?.map(|m| m.id);
        tx.execute("INSERT INTO listens(media_id,title,artist,album,duration_ms,started_at,listened_ms) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(title,artist,album,started_at) DO UPDATE SET listened_ms=MAX(listens.listened_ms,excluded.listened_ms)",params![media_id,listen.title,listen.artist,listen.album,sql_int(listen.duration_ms)?,listen.started_at,sql_int(listen.listened_ms)?])?;
        let id = tx.query_row(
            "SELECT id FROM listens WHERE title=?1 AND artist=?2 AND album=?3 AND started_at=?4",
            params![listen.title, listen.artist, listen.album, listen.started_at],
            |r| r.get(0),
        )?;
        for service in services {
            tx.execute(
                "INSERT OR IGNORE INTO scrobble_outbox(service_id,listen_id) VALUES(?1,?2)",
                params![service, id],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }
    pub fn scrobbles(&self, now: i64) -> Result<Vec<(i64, String, crate::scrobble::Listen)>> {
        let mut q=self.conn.prepare("SELECT o.id,o.service_id,l.media_id,l.title,l.artist,l.album,l.duration_ms,l.started_at,l.listened_ms FROM scrobble_outbox o JOIN listens l ON l.id=o.listen_id WHERE o.next_attempt<=?1 ORDER BY o.id LIMIT 10")?;
        Ok(q.query_map([now], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                crate::scrobble::Listen {
                    media_id: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    title: r.get(3)?,
                    artist: r.get(4)?,
                    album: r.get(5)?,
                    duration_ms: r.get::<_, i64>(6)? as u64,
                    started_at: r.get(7)?,
                    listened_ms: r.get::<_, i64>(8)? as u64,
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn scrobble_result(&self, id: i64, success: bool, now: i64) -> Result<()> {
        if success {
            self.conn
                .execute("DELETE FROM scrobble_outbox WHERE id=?1", [id])?;
        } else {
            self.conn.execute("UPDATE scrobble_outbox SET attempts=attempts+1,next_attempt=?2+MIN(3600,30*(1<<MIN(attempts,7))) WHERE id=?1",params![id,now])?;
        }
        Ok(())
    }
    pub fn pending_scrobbles(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM scrobble_outbox", [], |r| {
                r.get::<_, i64>(0)
            })? as u64)
    }
    pub fn remove_scrobbles(&self, service: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM scrobble_outbox WHERE service_id=?1", [service])?;
        Ok(())
    }
    pub fn listening_history(&self) -> Result<Vec<crate::scrobble::Listen>> {
        let mut q=self.conn.prepare("SELECT media_id,title,artist,album,duration_ms,started_at,listened_ms FROM listens ORDER BY started_at DESC LIMIT 100000")?;
        Ok(q.query_map([], |r| {
            Ok(crate::scrobble::Listen {
                media_id: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                title: r.get(1)?,
                artist: r.get(2)?,
                album: r.get(3)?,
                duration_ms: r.get::<_, i64>(4)? as u64,
                started_at: r.get(5)?,
                listened_ms: r.get::<_, i64>(6)? as u64,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn analytics(&self) -> Result<Vec<(String, u64, u64)>> {
        let mut q=self.conn.prepare("SELECT CASE WHEN artist='' THEN 'Unknown artist' ELSE artist END,COUNT(*),SUM(listened_ms) FROM listens GROUP BY artist ORDER BY SUM(listened_ms) DESC LIMIT 30")?;
        Ok(q.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get::<_, i64>(1)? as u64,
                r.get::<_, i64>(2)? as u64,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn rename_media(&self, id: &str, title: &str) -> Result<()> {
        if title.trim().is_empty() {
            bail!("Enter a title");
        }
        self.conn.execute(
            "UPDATE media SET title=?2 WHERE id=?1",
            params![id, title.trim()],
        )?;
        Ok(())
    }
    pub fn remove_media(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM media WHERE id=?1", [id])?;
        Ok(())
    }
    pub fn unsubscribe(&self, url: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM subscriptions WHERE url=?1", [url])?;
        Ok(())
    }
    pub fn downloaded_items(&self) -> Result<Vec<Media>> {
        let mut q=self.conn.prepare("SELECT m.id,m.kind,m.title,m.artist,m.album,m.location,m.duration_ms,m.artwork,m.favorite,m.provider,m.group_id,m.track_number FROM media m JOIN downloads d ON d.media_id=m.id ORDER BY m.album,m.track_number LIMIT 100")?;
        Ok(q.query_map([], decode_media)?
            .collect::<rusqlite::Result<_>>()?)
    }
}
fn parse_kind(s: &str) -> MediaKind {
    match s {
        "online_music" => MediaKind::OnlineMusic,
        "audiobook" => MediaKind::Audiobook,
        "podcast" => MediaKind::Podcast,
        "radio" => MediaKind::Radio,
        "ambient" => MediaKind::Ambient,
        "recording" => MediaKind::Recording,
        _ => MediaKind::LocalMusic,
    }
}
pub(crate) fn decode_media(r: &rusqlite::Row<'_>) -> rusqlite::Result<Media> {
    let k: String = r.get(1)?;
    Ok(Media {
        id: r.get(0)?,
        kind: parse_kind(&k),
        title: r.get(2)?,
        artist: r.get(3)?,
        album: r.get(4)?,
        location: r.get(5)?,
        duration_ms: r.get::<_, i64>(6)?.max(0) as u64,
        artwork: r.get(7)?,
        favorite: r.get(8)?,
        provider: r.get(9)?,
        group_id: r.get(10)?,
        track_number: r.get(11)?,
    })
}

fn sql_int(n: u64) -> Result<i64> {
    Ok(i64::try_from(n)?)
}

#[cfg(test)]
mod performance_regression_tests {
    use super::*;
    #[test]
    fn prepared_settings_observe_updates_and_deletions() {
        let db = Store::memory().unwrap();
        assert_eq!(db.setting("theme").unwrap(), None);
        db.set("theme", "black").unwrap();
        assert_eq!(db.setting("theme").unwrap().as_deref(), Some("black"));
        db.conn
            .execute("UPDATE settings SET value='white' WHERE key='theme'", [])
            .unwrap();
        assert_eq!(db.setting("theme").unwrap().as_deref(), Some("white"));
        db.conn
            .execute("DELETE FROM settings WHERE key='theme'", [])
            .unwrap();
        assert_eq!(db.setting("theme").unwrap(), None);
    }
    #[test]
    fn batched_flags_preserve_malformed_path_behavior() {
        let mut db = Store::memory().unwrap();
        let m = Media::new(MediaKind::LocalMusic, "local", "/audio.wav", "Audio");
        db.upsert(std::slice::from_ref(&m)).unwrap();
        for value in [
            rusqlite::types::Value::Blob(vec![0xff]),
            rusqlite::types::Value::Integer(12),
        ] {
            db.conn
                .execute(
                    "INSERT OR REPLACE INTO downloads(media_id,path) VALUES(?1,?2)",
                    params![m.id, value],
                )
                .unwrap();
            let expected = db.download_path(&m.id).ok().flatten().is_some();
            assert_eq!(
                db.download_flags(&[&m.id]).unwrap().contains(&m.id),
                expected
            );
        }
        db.conn
            .execute("UPDATE downloads SET path=CAST(x'ff' AS TEXT)", [])
            .unwrap();
        assert!(db.download_path(&m.id).is_err());
        assert!(db.download_flags(&[&m.id]).unwrap().is_empty());
    }
    #[test]
    fn revision_maximum_uses_the_counter_index() {
        let db = Store::memory().unwrap();
        let detail: String = db
            .conn
            .query_row(
                "EXPLAIN QUERY PLAN SELECT MAX(counter) FROM sync_records",
                [],
                |r| r.get(3),
            )
            .unwrap();
        assert!(detail.contains("sync_records_counter"), "{detail}");
    }
}
