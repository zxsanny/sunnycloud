-- Owner: components/20_vault_service. Owner decision 2026-11-13: when 02 purges a tombstoned vault root after the
-- retention window, the vault row goes with it (its files are purged in the same run), so the purge never fails on
-- this foreign key and the account can create a new vault.
ALTER TABLE vaults DROP CONSTRAINT vaults_root_node_id_account_id_fkey;

ALTER TABLE vaults
    ADD CONSTRAINT vaults_root_node_id_account_id_fkey FOREIGN KEY (root_node_id, account_id)
    REFERENCES nodes (id, account_id) ON DELETE CASCADE;
