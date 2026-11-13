ALTER TABLE vaults DROP CONSTRAINT vaults_root_node_id_account_id_fkey;

ALTER TABLE vaults
    ADD CONSTRAINT vaults_root_node_id_account_id_fkey FOREIGN KEY (root_node_id, account_id)
    REFERENCES nodes (id, account_id);
