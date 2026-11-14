-- Owner: components/02_tree (the column) with components/20_vault_service (the key it references). Vault
-- membership (E-30/c01, AZ-3913; wired by AZ-3927): a node is in a vault when vault_id is set. 20 sets it on the vault
-- root inside the transaction that creates the vault (through 02's VaultRoots port, which locks the root and checks
-- it is still empty); every node created later copies it from its parent and nothing changes it afterwards, so
-- "is this node in a vault" is read from the node row alone, with no ancestry walk and no call to 20 (ADR-015).
-- The reference names (vault_id, account_id), so a node can only be in a vault of its own account. Deleting a vault
-- row (account erase runs 20's hook before 02's; a purged vault root cascades its vault row) clears only vault_id;
-- the nodes then go with 02's own erase or purge. The partial index serves that clearing and keeps it off a scan of
-- every node.
ALTER TABLE vaults ADD CONSTRAINT vaults_id_account_id_key UNIQUE (id, account_id);

ALTER TABLE nodes ADD COLUMN vault_id uuid;

ALTER TABLE nodes
    ADD CONSTRAINT nodes_vault_id_account_id_fkey FOREIGN KEY (vault_id, account_id)
    REFERENCES vaults (id, account_id) ON DELETE SET NULL (vault_id);

CREATE INDEX nodes_vault_idx ON nodes (vault_id) WHERE vault_id IS NOT NULL;
