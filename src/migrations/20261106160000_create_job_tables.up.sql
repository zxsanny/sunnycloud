-- Owner: components/08_media_jobs (domain word job). The three tables of the media pipeline (data_model 9.1 to
-- 9.3, 12.3). Ids and timestamps come from the application, so no id or time column has a default. A job names its
-- revision through (revision_id, account_id), so it can never point at another account's revision, and removing
-- the revision removes its jobs. derived references the blob through (blob_id, account_id); the blob owner cleanup
-- removes the derived row before the blob row. The claim runs in system scope across accounts, so jobs carries
-- the tenant policy with the system branch (fail closed for a request without a scope); worker_status is
-- installation-level and has no account column and no row-level security. Row-level security is ENABLE, no FORCE:
-- the owner (sunnycloud_migrator) bypasses it, the API role does not. The runtime role gets DML only (ADR-014).
-- jobs churns (about 20 k rows a day, each updated on claim, lease and finish), so it is tuned for dead tuples:
-- fillfactor leaves room on the page and autovacuum runs at 2 percent dead rows instead of 20.
CREATE TABLE jobs (
    id uuid NOT NULL,
    account_id uuid NOT NULL,
    revision_id uuid NOT NULL,
    kind text NOT NULL,
    lane text GENERATED ALWAYS AS (CASE WHEN kind = 'stills' THEN 'image' ELSE 'video' END) STORED NOT NULL,
    content_hash bytea NOT NULL,
    params_version smallint NOT NULL,
    priority smallint NOT NULL,
    status text NOT NULL,
    attempts smallint NOT NULL DEFAULT 0,
    available_at timestamptz NOT NULL,
    leased_by text,
    outcome text,
    last_error text,
    created_at timestamptz NOT NULL,
    finished_at timestamptz,
    CONSTRAINT jobs_pkey PRIMARY KEY (id),
    CONSTRAINT jobs_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT jobs_revision_id_account_id_fkey FOREIGN KEY (revision_id, account_id) REFERENCES revisions (id, account_id) ON DELETE CASCADE,
    CONSTRAINT jobs_key UNIQUE (account_id, kind, content_hash, params_version),
    CONSTRAINT jobs_kind_check CHECK (kind IN ('stills', 'poster', 'transcode')),
    CONSTRAINT jobs_content_hash_check CHECK (octet_length(content_hash) = 32),
    CONSTRAINT jobs_priority_check CHECK (priority IN (0, 1)),
    CONSTRAINT jobs_status_check CHECK (status IN ('queued', 'running', 'succeeded', 'dead')),
    CONSTRAINT jobs_attempts_check CHECK (attempts >= 0),
    CONSTRAINT jobs_outcome_check CHECK (outcome IN ('derived', 'skipped_compliant', 'obsolete')),
    CONSTRAINT jobs_last_error_check CHECK (char_length(last_error) <= 500)
) WITH (fillfactor = 70, autovacuum_vacuum_scale_factor = 0.02, autovacuum_analyze_scale_factor = 0.02);
CREATE INDEX jobs_claim_idx ON jobs (lane, priority, available_at) WHERE status IN ('queued', 'running');
CREATE INDEX jobs_revision_id_idx ON jobs (revision_id);
CREATE INDEX jobs_finished_idx ON jobs (finished_at) WHERE status IN ('succeeded', 'dead');
CREATE INDEX jobs_queued_idx ON jobs (account_id) WHERE status = 'queued';

CREATE TABLE derived (
    account_id uuid NOT NULL,
    content_hash bytea NOT NULL,
    kind text NOT NULL,
    params_version smallint NOT NULL,
    blob_id uuid NOT NULL,
    media_type text NOT NULL,
    size bigint NOT NULL,
    width integer,
    height integer,
    duration_ms integer,
    created_at timestamptz NOT NULL,
    CONSTRAINT derived_pkey PRIMARY KEY (account_id, content_hash, kind, params_version),
    CONSTRAINT derived_blob_id_key UNIQUE (blob_id),
    CONSTRAINT derived_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT derived_blob_id_account_id_fkey FOREIGN KEY (blob_id, account_id) REFERENCES blobs (id, account_id) ON DELETE RESTRICT,
    CONSTRAINT derived_kind_check CHECK (kind IN ('thumb', 'preview', 'transcode')),
    CONSTRAINT derived_content_hash_check CHECK (octet_length(content_hash) = 32),
    CONSTRAINT derived_size_check CHECK (size >= 0)
);

CREATE TABLE worker_status (
    worker_id text NOT NULL,
    seen_at timestamptz NOT NULL,
    image_age_days integer NOT NULL,
    decoder_ok boolean NOT NULL,
    CONSTRAINT worker_status_pkey PRIMARY KEY (worker_id)
);

ALTER TABLE jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE derived ENABLE ROW LEVEL SECURITY;

CREATE POLICY jobs_tenant ON jobs USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);
CREATE POLICY derived_tenant ON derived USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);

GRANT SELECT, INSERT, UPDATE, DELETE ON jobs, derived, worker_status TO sunnycloud;
