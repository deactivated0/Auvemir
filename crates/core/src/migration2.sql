BEGIN IMMEDIATE;
CREATE TABLE listens(id INTEGER PRIMARY KEY,media_id TEXT REFERENCES media(id) ON DELETE SET NULL,title TEXT NOT NULL,artist TEXT NOT NULL,album TEXT NOT NULL,duration_ms INTEGER NOT NULL,started_at INTEGER NOT NULL,listened_ms INTEGER NOT NULL,UNIQUE(media_id,started_at));
CREATE INDEX listens_started ON listens(started_at DESC);
CREATE TABLE scrobble_outbox(id INTEGER PRIMARY KEY,service_id TEXT NOT NULL,listen_id INTEGER NOT NULL REFERENCES listens(id) ON DELETE CASCADE,attempts INTEGER NOT NULL DEFAULT 0,next_attempt INTEGER NOT NULL DEFAULT 0,UNIQUE(service_id,listen_id));
CREATE INDEX scrobble_ready ON scrobble_outbox(next_attempt);
PRAGMA user_version=2;
COMMIT;
