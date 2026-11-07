-- Owner: components/06_sharing (domain word share). One table, share_links (data_model 8.1, 12.3), sorted after
-- 02's tree migration because its composite foreign key names nodes. Ids and timestamps come from the application,
-- so no id or time column has a default. Only the SHA-256 of the token is stored. A link names its target through
-- (node_id, account_id), so it can never point at another account's node, and purging the node removes its links
-- (ON DELETE CASCADE; the cascade is not subject to row-level security). The token lookup runs in system scope
-- before the owner is known, so the policy carries the system branch (fail closed for a request without a scope).
-- Row-level security is ENABLE, no FORCE: the owner (sunnycloud_migrator) bypasses it, the API role does not.
-- The runtime role gets DML only (ADR-014).
CREATE TABLE share_links (
    id uuid NOT NULL,
    account_id uuid NOT NULL,
    node_id uuid NOT NULL,
    token_hash bytea NOT NULL,
    password_hash text,
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL,
    CONSTRAINT share_links_pkey PRIMARY KEY (id),
    CONSTRAINT share_links_token_hash_key UNIQUE (token_hash),
    CONSTRAINT share_links_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT share_links_node_id_account_id_fkey FOREIGN KEY (node_id, account_id) REFERENCES nodes (id, account_id) ON DELETE CASCADE,
    CONSTRAINT share_links_token_hash_check CHECK (octet_length(token_hash) = 32)
);
CREATE INDEX share_links_account_id_idx ON share_links (account_id);
CREATE INDEX share_links_node_id_idx ON share_links (node_id);

ALTER TABLE share_links ENABLE ROW LEVEL SECURITY;

CREATE POLICY share_links_tenant ON share_links USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);

GRANT SELECT, INSERT, UPDATE, DELETE ON share_links TO sunnycloud;
