DROP INDEX sessions_prev_refresh_hash_idx;

ALTER TABLE sessions DROP COLUMN successor_enc;
