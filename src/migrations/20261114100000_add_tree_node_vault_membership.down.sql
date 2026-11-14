DROP INDEX nodes_vault_idx;

ALTER TABLE nodes DROP CONSTRAINT nodes_vault_id_account_id_fkey;

ALTER TABLE nodes DROP COLUMN vault_id;

ALTER TABLE vaults DROP CONSTRAINT vaults_id_account_id_key;
