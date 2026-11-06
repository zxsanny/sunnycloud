-- Forward-only: this file is never edited once released; a schema change is the next numbered file.
-- entry.rel_path is the lookup form of the path (the path itself on a case-sensitive pair, its components
-- folded by name_key on a case-insensitive one); disk_path holds the path as it is on disk when that differs.
CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE pair (
    id INTEGER PRIMARY KEY,
    server_node_id BLOB NOT NULL,
    local_root TEXT NOT NULL,
    rules TEXT NOT NULL,
    root_sentinel TEXT NOT NULL,
    case_insensitive INTEGER NOT NULL,
    state TEXT NOT NULL,
    hold_detail TEXT
);

CREATE TABLE entry (
    pair_id INTEGER NOT NULL REFERENCES pair (id) ON DELETE CASCADE,
    rel_path TEXT NOT NULL,
    disk_path TEXT,
    node_id BLOB,
    revision_id BLOB,
    content_hash BLOB,
    size INTEGER NOT NULL,
    mtime_ns INTEGER NOT NULL,
    file_id TEXT NOT NULL,
    state TEXT NOT NULL,
    settle_until INTEGER,
    escaped_from TEXT,
    PRIMARY KEY (pair_id, rel_path)
) WITHOUT ROWID;

CREATE INDEX entry_file_id ON entry (pair_id, file_id);
CREATE INDEX entry_node_id ON entry (pair_id, node_id);
CREATE INDEX entry_not_synced ON entry (pair_id, state) WHERE state <> 'synced';

-- Small-file batches staged but not yet resolved (sync-core BatchRecord); entries is the JSON list.
CREATE TABLE batch (
    commit_id TEXT PRIMARY KEY,
    entries TEXT NOT NULL
) WITHOUT ROWID;

-- Resume state of one large upload (sync-core UploadCheckpoint); local_key is the opaque key the core passes.
-- blocklist is the 32-byte block hashes in order and doubles as the incremental hash state.
CREATE TABLE upload_checkpoint (
    local_key TEXT PRIMARY KEY,
    session_id TEXT,
    commit_id TEXT NOT NULL,
    file_hash BLOB,
    state TEXT NOT NULL,
    blocklist BLOB NOT NULL
) WITHOUT ROWID;
