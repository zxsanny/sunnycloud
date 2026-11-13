-- Owner: components/03_transfer. Lost-commit recovery (owner decision 2026-11-13, AZ-3949 option a): a large
-- upload's session row stays after its commit as a short record of the revision the commit created, until
-- expires_at (commit time + upload_session_ttl), so start_upload with the same (account, client_key) answers
-- NothingToSend{revision} whatever the name holds now. NULL means the session is open. A committed row holds no
-- reservation (reserved_bytes = 0), does not count toward the open-session cap, and the reaper deletes it without
-- touching its blob. The revision is referenced through the composite key, so it can only be the account's own;
-- removing the revision (retention purge, account erase) deletes the committed record with it: clearing only the
-- pointer would turn it back into an open-looking session.
ALTER TABLE upload_sessions ADD COLUMN committed_revision_id uuid;
ALTER TABLE upload_sessions ADD CONSTRAINT upload_sessions_committed_revision_fkey
    FOREIGN KEY (committed_revision_id, account_id) REFERENCES revisions (id, account_id)
    ON DELETE CASCADE;
