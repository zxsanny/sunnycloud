-- Jobs of a collected revision would have been deleted with it under the cascade, so they go before NOT NULL.
ALTER TABLE jobs DROP CONSTRAINT jobs_revision_id_account_id_fkey;

DELETE FROM jobs WHERE revision_id IS NULL;

ALTER TABLE jobs ALTER COLUMN revision_id SET NOT NULL;

ALTER TABLE jobs
    ADD CONSTRAINT jobs_revision_id_account_id_fkey FOREIGN KEY (revision_id, account_id)
    REFERENCES revisions (id, account_id) ON DELETE CASCADE;
