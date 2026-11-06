-- Owner: components/03_transfer (domain word transfer). The two tables of the transfer component
-- (data_model 6.3, 7.3, 12.3). Ids and timestamps come from the application, so no column has a default.
-- upload_sessions holds one row per open large upload or staged small upload; it references its staging blob
-- through the composite key (blob_id, account_id), so a session can never name another account's blob, and the
-- blob row cannot be removed while its session exists (the session is deleted first). The block count follows
-- from the declared size (2 MiB blocks, a protocol constant) and fixes the length of both byte arrays: one bit
-- per block in received (bit i is bit i % 8, least significant first, of byte i / 8) and 32 bytes per block in
-- block_hashes. commit_receipts holds the stored outcome of a batch commit under its commit id.
-- Row-level security (ENABLE, no FORCE) covers both tables; the owner (sunnycloud_migrator) bypasses it, the
-- API role does not. The runtime role gets DML only (ADR-014).
CREATE TABLE upload_sessions (
    id uuid NOT NULL,
    account_id uuid NOT NULL,
    blob_id uuid NOT NULL,
    client_key text NOT NULL,
    declared_size bigint NOT NULL,
    block_count integer NOT NULL,
    reserved_bytes bigint NOT NULL,
    received bytea NOT NULL,
    block_hashes bytea NOT NULL,
    created_at timestamptz NOT NULL,
    last_checkpoint_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    CONSTRAINT upload_sessions_pkey PRIMARY KEY (id),
    CONSTRAINT upload_sessions_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT upload_sessions_blob_fkey FOREIGN KEY (blob_id, account_id) REFERENCES blobs (id, account_id) ON DELETE RESTRICT,
    CONSTRAINT upload_sessions_blob_id_key UNIQUE (blob_id),
    CONSTRAINT upload_sessions_account_id_client_key_key UNIQUE (account_id, client_key),
    CONSTRAINT upload_sessions_client_key_check CHECK (octet_length(client_key) BETWEEN 1 AND 256),
    CONSTRAINT upload_sessions_declared_size_check CHECK (declared_size >= 0),
    CONSTRAINT upload_sessions_reserved_bytes_check CHECK (reserved_bytes >= 0),
    CONSTRAINT upload_sessions_block_count_check CHECK (
        block_count = declared_size / 2097152 + (declared_size % 2097152 > 0)::int
    ),
    CONSTRAINT upload_sessions_received_check CHECK (octet_length(received) = (block_count + 7) / 8),
    CONSTRAINT upload_sessions_block_hashes_check CHECK (octet_length(block_hashes) = 32 * block_count)
);
CREATE INDEX upload_sessions_expires_at_idx ON upload_sessions (expires_at);

CREATE TABLE commit_receipts (
    account_id uuid NOT NULL,
    commit_id uuid NOT NULL,
    request_hash bytea NOT NULL,
    response jsonb NOT NULL,
    created_at timestamptz NOT NULL,
    CONSTRAINT commit_receipts_pkey PRIMARY KEY (account_id, commit_id),
    CONSTRAINT commit_receipts_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT commit_receipts_request_hash_check CHECK (octet_length(request_hash) = 32)
);
CREATE INDEX commit_receipts_created_at_idx ON commit_receipts (created_at);

ALTER TABLE upload_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE commit_receipts ENABLE ROW LEVEL SECURITY;

CREATE POLICY upload_sessions_tenant ON upload_sessions USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);
CREATE POLICY commit_receipts_tenant ON commit_receipts USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);

GRANT SELECT, INSERT, UPDATE, DELETE ON upload_sessions, commit_receipts TO sunnycloud;
