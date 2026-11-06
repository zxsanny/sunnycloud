-- Owner: components/04_blobstore (domain word blob). The two tables of the blob store (data_model 6.1, 6.2, 12.3).
-- Ids and timestamps come from the application, so no column has a default. A blob row is created in state
-- uploading (size not yet known) or committed (size known); gc_pending is written together with its blob_gc row.
-- blob_gc references blobs through the composite key (id, account_id), so a queue entry can never name another
-- account's blob; removing the blobs row removes its queue entry. Row-level security (ENABLE, no FORCE) covers
-- both tables; the owner (sunnycloud_migrator) bypasses it, the API role does not.
CREATE TABLE blobs (
    id uuid NOT NULL,
    account_id uuid NOT NULL,
    state text NOT NULL,
    size bigint,
    wrapped_key bytea NOT NULL,
    master_key_id smallint NOT NULL,
    created_at timestamptz NOT NULL,
    last_scrubbed_at timestamptz,
    unavailable_at timestamptz,
    CONSTRAINT blobs_pkey PRIMARY KEY (id),
    CONSTRAINT blobs_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT blobs_id_account_id_key UNIQUE (id, account_id),
    CONSTRAINT blobs_state_check CHECK (state IN ('uploading', 'committed', 'gc_pending')),
    CONSTRAINT blobs_size_check CHECK (size >= 0),
    CONSTRAINT blobs_committed_size_check CHECK (state <> 'committed' OR size IS NOT NULL)
);
CREATE INDEX blobs_account_id_idx ON blobs (account_id);
CREATE INDEX blobs_scrub_idx ON blobs (last_scrubbed_at NULLS FIRST, id) WHERE state = 'committed';

CREATE TABLE blob_gc (
    blob_id uuid NOT NULL,
    account_id uuid NOT NULL,
    due_at timestamptz NOT NULL,
    reason text NOT NULL,
    CONSTRAINT blob_gc_pkey PRIMARY KEY (blob_id),
    CONSTRAINT blob_gc_blob_id_account_id_fkey FOREIGN KEY (blob_id, account_id) REFERENCES blobs (id, account_id) ON DELETE CASCADE,
    CONSTRAINT blob_gc_reason_check CHECK (reason IN ('replaced', 'deleted', 'account_deleted', 'derived_removed'))
);
CREATE INDEX blob_gc_due_at_idx ON blob_gc (due_at);

ALTER TABLE blobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE blob_gc ENABLE ROW LEVEL SECURITY;

CREATE POLICY blobs_tenant ON blobs USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);
CREATE POLICY blob_gc_tenant ON blob_gc USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);

GRANT SELECT, INSERT, UPDATE, DELETE ON blobs, blob_gc TO sunnycloud;
