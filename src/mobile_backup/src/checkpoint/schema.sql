-- Schema version 1 of the checkpoint database (17 §4). `PRAGMA user_version` holds the version.
CREATE TABLE asset (
    asset_id INTEGER PRIMARY KEY,
    change_token TEXT NOT NULL,
    size INTEGER NOT NULL,
    date_taken INTEGER NOT NULL,
    content_hash BLOB CHECK (content_hash IS NULL OR length(content_hash) = 32),
    upload_name TEXT,
    client_key BLOB CHECK (client_key IS NULL OR length(client_key) = 16),
    node_id BLOB CHECK (node_id IS NULL OR length(node_id) = 16),
    revision_id BLOB CHECK (revision_id IS NULL OR length(revision_id) = 16),
    state TEXT NOT NULL DEFAULT 'new' CHECK (state IN
        ('new', 'hashing', 'uploading', 'uploaded', 'skipped_gone', 'skipped_name', 'skipped_size', 'failed')),
    linked_from INTEGER
);

CREATE TABLE cursor (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

-- The resume record of one large upload, keyed by the host's `local_key` (13 `UploadCheckpoint`).
CREATE TABLE session (
    local_key TEXT PRIMARY KEY,
    state TEXT NOT NULL CHECK (state IN ('hashing', 'uploading', 'committing')),
    session_id BLOB CHECK (session_id IS NULL OR length(session_id) = 16),
    file_hash BLOB CHECK (file_hash IS NULL OR length(file_hash) = 32),
    commit_id BLOB NOT NULL CHECK (length(commit_id) = 16),
    block_hashes BLOB NOT NULL CHECK (length(block_hashes) % 32 = 0)
) WITHOUT ROWID;

-- A staged small-file batch, written before its first `stage_small` and deleted after its results are stored.
CREATE TABLE batch (
    commit_id BLOB PRIMARY KEY CHECK (length(commit_id) = 16),
    entries TEXT NOT NULL
) WITHOUT ROWID;
