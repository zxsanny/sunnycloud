-- Owner: components/01_identity. Device approval binding (AZ-4267, owner decision 2026-11-13): the first account that
-- looks up a pending grant's user code is recorded on the grant, and only that account may approve or deny it;
-- another account naming the grant id, or looking up the same code afterwards, gets the answer of an unknown id or
-- code. NULL until the first lookup. A grant of an erased account goes with it.
ALTER TABLE device_grants ADD COLUMN looked_up_by_account_id uuid;
ALTER TABLE device_grants ADD CONSTRAINT device_grants_looked_up_by_account_id_fkey
    FOREIGN KEY (looked_up_by_account_id) REFERENCES accounts (id) ON DELETE CASCADE;
