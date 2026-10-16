BEGIN IMMEDIATE;
CREATE UNIQUE INDEX listens_identity ON listens(title,artist,album,started_at);
PRAGMA user_version=3;
COMMIT;
