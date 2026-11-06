-- Owner: components/01_identity (domain word identity). A refresh that repeats the previous token inside the
-- grace window (a lost response) is answered with the same successor pair. successor_enc holds that pair sealed
-- under a key derived from the previous refresh token, which only the client and the request that carries it
-- know, so the database alone cannot read it. It is replaced by the next rotation and cleared on revocation.
-- The partial index serves the lookup by previous hash that every unknown refresh token reaches.
ALTER TABLE sessions ADD COLUMN successor_enc bytea;

CREATE INDEX sessions_prev_refresh_hash_idx ON sessions (prev_refresh_hash) WHERE prev_refresh_hash IS NOT NULL;
