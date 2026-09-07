-- Single-row canary table recording when the latest backup was taken.
-- Schema/columns mirror github.com/suhlig/sqlite-vault so backups stay
-- interchangeable with that tooling. Written by the backup task before each
-- VACUUM INTO snapshot; read by `kehrkraft verify` for freshness checks.
CREATE TABLE IF NOT EXISTS backup_canary (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    job_id       TEXT NOT NULL,
    backed_up_at TEXT NOT NULL
);