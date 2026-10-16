BEGIN IMMEDIATE;
ALTER TABLE playlists ADD COLUMN sync_id TEXT;
CREATE UNIQUE INDEX playlists_sync_id ON playlists(sync_id) WHERE sync_id IS NOT NULL;
CREATE TABLE sync_records(key TEXT PRIMARY KEY, counter INTEGER NOT NULL, device TEXT NOT NULL, value TEXT NOT NULL);
CREATE TABLE sync_known_media(identity TEXT PRIMARY KEY);
PRAGMA user_version=4;
COMMIT;
