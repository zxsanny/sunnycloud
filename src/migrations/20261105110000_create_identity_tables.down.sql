DROP TABLE device_grants;
DROP TABLE login_codes;
DROP TABLE oauth_states;
DROP TABLE invites;
DROP TABLE device_tokens;
DROP TABLE sessions;
DROP TABLE identities;

DROP INDEX accounts_state_idx;
ALTER TABLE accounts
    DROP CONSTRAINT accounts_role_check,
    DROP COLUMN deletion_requested_at,
    DROP COLUMN role,
    DROP COLUMN display_name;
