-- Owner: components/08_media_jobs. Owner decision 2026-11-18 (AZ-4332, worker_job_protocol.md): a job outlives
-- its collected revision, so the worker's input and output answer 404 revision_obsolete and the supervisor closes
-- the job `obsolete`. Collecting the revision clears only revision_id (PostgreSQL 15+ column list); account_id
-- stays, so the account FK and the erase hook still own the row. A NULL revision_id is the obsolete mark.
ALTER TABLE jobs DROP CONSTRAINT jobs_revision_id_account_id_fkey;

ALTER TABLE jobs ALTER COLUMN revision_id DROP NOT NULL;

ALTER TABLE jobs
    ADD CONSTRAINT jobs_revision_id_account_id_fkey FOREIGN KEY (revision_id, account_id)
    REFERENCES revisions (id, account_id) ON DELETE SET NULL (revision_id);
