-- Owner: components/07a_quota (domain word quota). One ledger row per account (data_model 8.2, 12.3).
-- limit_bytes NULL means the installation default (default_quota); the row is created and erased through the
-- account lifecycle hook, so there are no defaults for account_id and no cascade: erasing an account deletes
-- its quota row first.
-- Row-level security (ENABLE, no FORCE) as for the identity tables: the owner (sunnycloud_migrator) bypasses it,
-- the API role does not. The runtime role gets DML only (ADR-014).
CREATE TABLE quota (
    account_id uuid NOT NULL,
    limit_bytes bigint,
    used_bytes bigint NOT NULL DEFAULT 0,
    reserved_bytes bigint NOT NULL DEFAULT 0,
    CONSTRAINT quota_pkey PRIMARY KEY (account_id),
    CONSTRAINT quota_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT quota_bytes_check CHECK (used_bytes >= 0 AND reserved_bytes >= 0)
);

ALTER TABLE quota ENABLE ROW LEVEL SECURITY;

CREATE POLICY quota_tenant ON quota USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);

GRANT SELECT, INSERT, UPDATE, DELETE ON quota TO sunnycloud;
