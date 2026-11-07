-- Owner: components/05_sync_feed (domain word feed). The per-account feed counter (data_model 7.1) and, on the
-- tree's `nodes` table, the stamp column and its index; `nodes.seq` is written only by the feed stamp. This
-- migration sorts after 02's tree migration and adds nothing else to `nodes`. Ids come from the application, so
-- epoch has no default; seq and purge_horizon start at 0. The row is separate from accounts and quota so the
-- commit-path lock is not shared with profile or reservation updates.
-- Row-level security (ENABLE, no FORCE) covers account_feeds; the owner (sunnycloud_migrator) bypasses it, the
-- API role does not. The runtime role gets DML only (ADR-014).
CREATE TABLE account_feeds (
    account_id uuid NOT NULL,
    epoch bigint NOT NULL,
    seq bigint NOT NULL DEFAULT 0,
    purge_horizon bigint NOT NULL DEFAULT 0,
    CONSTRAINT account_feeds_pkey PRIMARY KEY (account_id),
    CONSTRAINT account_feeds_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT
);

ALTER TABLE account_feeds ENABLE ROW LEVEL SECURITY;

CREATE POLICY account_feeds_tenant ON account_feeds USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);

GRANT SELECT, INSERT, UPDATE, DELETE ON account_feeds TO sunnycloud;

ALTER TABLE nodes ADD COLUMN seq bigint NOT NULL DEFAULT 0;
CREATE INDEX nodes_feed_idx ON nodes (account_id, seq);
