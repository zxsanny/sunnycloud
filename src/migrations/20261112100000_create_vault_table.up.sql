-- Owner: components/20_vault_service (domain word vault, v1.2, additive). One table, vaults (data_model 10.2,
-- 12.3), sorted after 02's tree migration because its composite foreign key names nodes. Ids and timestamps come
-- from the application, so no id or time column has a default. One vault per account (owner decision 2026-11-11):
-- account_id is UNIQUE and a second create is refused by the service as VaultExists. The root is named through
-- (root_node_id, account_id), so it can never be a node of another account; that it is a folder, and empty at
-- creation, is the service's rule. The header is opaque to the server (authenticated by the client), so only the
-- column type is fixed here; its size bound is the max_vault_header_bytes setting, applied by the service.
-- The root foreign key has no delete rule: account erase runs 20's hook before 02's (decision 2026-11-05), and
-- purging a vault root while its vault row exists is refused until 20 deletes the vault.
-- Row-level security is ENABLE, no FORCE: the owner (sunnycloud_migrator) bypasses it, the API role does not.
-- The runtime role gets DML only (ADR-014).
CREATE TABLE vaults (
    id uuid NOT NULL,
    account_id uuid NOT NULL,
    root_node_id uuid NOT NULL,
    header bytea NOT NULL,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT vaults_pkey PRIMARY KEY (id),
    CONSTRAINT vaults_account_id_key UNIQUE (account_id),
    CONSTRAINT vaults_root_node_id_key UNIQUE (root_node_id),
    CONSTRAINT vaults_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT vaults_root_node_id_account_id_fkey FOREIGN KEY (root_node_id, account_id) REFERENCES nodes (id, account_id)
);

ALTER TABLE vaults ENABLE ROW LEVEL SECURITY;

CREATE POLICY vaults_tenant ON vaults USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);

GRANT SELECT, INSERT, UPDATE, DELETE ON vaults TO sunnycloud;
