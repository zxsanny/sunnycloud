-- Owner: components/01_identity (domain word identity). Adds the identity columns to the minimal accounts table
-- of accounts_base and creates the seven identity tables (data_model 4.1 to 4.8, 12.3). The new accounts columns
-- have no defaults: accounts are created only by the identity service, which supplies every value, so the table
-- is empty when this runs and a populated one fails loudly instead of receiving invented values.
-- Row-level security (ENABLE, no FORCE) covers the four account-scoped tables; the owner (sunnycloud_migrator)
-- bypasses it, the API role does not. invites, oauth_states and login_codes are resolved by hash before the
-- caller is known and are reachable in system scope only by convention, so they carry no policy.
ALTER TABLE accounts
    ADD COLUMN display_name text NOT NULL,
    ADD COLUMN role text NOT NULL,
    ADD COLUMN deletion_requested_at timestamptz,
    ADD CONSTRAINT accounts_role_check CHECK (role IN ('user', 'admin'));

CREATE INDEX accounts_state_idx ON accounts (state) WHERE state <> 'active';

CREATE TABLE identities (
    id uuid NOT NULL,
    account_id uuid NOT NULL,
    provider text NOT NULL,
    provider_user_id text NOT NULL,
    email text,
    email_verified boolean NOT NULL,
    created_at timestamptz NOT NULL,
    last_login_at timestamptz,
    CONSTRAINT identities_pkey PRIMARY KEY (id),
    CONSTRAINT identities_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT identities_provider_provider_user_id_key UNIQUE (provider, provider_user_id)
);
CREATE INDEX identities_account_id_idx ON identities (account_id);

CREATE TABLE sessions (
    id uuid NOT NULL,
    account_id uuid NOT NULL,
    client_kind text NOT NULL,
    label text NOT NULL,
    token_hash bytea NOT NULL,
    access_expires_at timestamptz NOT NULL,
    refresh_hash bytea NOT NULL,
    refresh_expires_at timestamptz NOT NULL,
    prev_refresh_hash bytea,
    rotated_at timestamptz,
    created_at timestamptz NOT NULL,
    created_ip inet,
    last_seen_at timestamptz,
    revoked_at timestamptz,
    revoke_reason text,
    CONSTRAINT sessions_pkey PRIMARY KEY (id),
    CONSTRAINT sessions_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT sessions_token_hash_key UNIQUE (token_hash),
    CONSTRAINT sessions_refresh_hash_key UNIQUE (refresh_hash),
    CONSTRAINT sessions_client_kind_check CHECK (client_kind IN ('web', 'android', 'ios')),
    CONSTRAINT sessions_token_hash_check CHECK (octet_length(token_hash) = 32),
    CONSTRAINT sessions_refresh_hash_check CHECK (octet_length(refresh_hash) = 32),
    CONSTRAINT sessions_prev_refresh_hash_check CHECK (octet_length(prev_refresh_hash) = 32),
    CONSTRAINT sessions_revoke_reason_check CHECK (revoke_reason IN ('user', 'admin', 'refresh_reuse', 'account_deleted'))
);
CREATE INDEX sessions_account_id_idx ON sessions (account_id);

CREATE TABLE device_tokens (
    id uuid NOT NULL,
    account_id uuid NOT NULL,
    token_hash bytea NOT NULL,
    device_name text NOT NULL,
    platform text NOT NULL,
    created_at timestamptz NOT NULL,
    last_seen_at timestamptz,
    revoked_at timestamptz,
    revoke_reason text,
    CONSTRAINT device_tokens_pkey PRIMARY KEY (id),
    CONSTRAINT device_tokens_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT device_tokens_token_hash_key UNIQUE (token_hash),
    CONSTRAINT device_tokens_token_hash_check CHECK (octet_length(token_hash) = 32),
    CONSTRAINT device_tokens_platform_check CHECK (platform IN ('linux', 'windows', 'macos')),
    CONSTRAINT device_tokens_revoke_reason_check CHECK (revoke_reason IN ('user', 'admin', 'refresh_reuse', 'account_deleted'))
);
CREATE INDEX device_tokens_account_id_idx ON device_tokens (account_id);

-- account_id and approved_by_session_id stay NULL until a session approves the grant. Finalizing an account
-- deletes its sessions before its grants, so the session reference is SET NULL, not a blocker.
CREATE TABLE device_grants (
    id uuid NOT NULL,
    device_code_hash bytea NOT NULL,
    user_code text NOT NULL,
    requester_ip inet NOT NULL,
    requester_location text,
    device_name text NOT NULL,
    platform text NOT NULL,
    status text NOT NULL,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    last_polled_at timestamptz,
    account_id uuid,
    approved_by_session_id uuid,
    approved_at timestamptz,
    CONSTRAINT device_grants_pkey PRIMARY KEY (id),
    CONSTRAINT device_grants_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT device_grants_approved_by_session_id_fkey FOREIGN KEY (approved_by_session_id) REFERENCES sessions (id) ON DELETE SET NULL,
    CONSTRAINT device_grants_device_code_hash_key UNIQUE (device_code_hash),
    CONSTRAINT device_grants_device_code_hash_check CHECK (octet_length(device_code_hash) = 32),
    CONSTRAINT device_grants_platform_check CHECK (platform IN ('linux', 'windows', 'macos')),
    CONSTRAINT device_grants_status_check CHECK (status IN ('pending', 'approved', 'denied', 'consumed'))
);
CREATE UNIQUE INDEX device_grants_user_code_key ON device_grants (user_code) WHERE status = 'pending';
CREATE INDEX device_grants_account_id_idx ON device_grants (account_id);

CREATE TABLE invites (
    id uuid NOT NULL,
    kind text NOT NULL,
    token_hash bytea NOT NULL,
    grants_role text NOT NULL DEFAULT 'user',
    target_account_id uuid,
    created_by uuid,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    redeemed_by uuid,
    redeemed_at timestamptz,
    note text,
    CONSTRAINT invites_pkey PRIMARY KEY (id),
    CONSTRAINT invites_target_account_id_fkey FOREIGN KEY (target_account_id) REFERENCES accounts (id) ON DELETE CASCADE,
    CONSTRAINT invites_created_by_fkey FOREIGN KEY (created_by) REFERENCES accounts (id) ON DELETE CASCADE,
    CONSTRAINT invites_redeemed_by_fkey FOREIGN KEY (redeemed_by) REFERENCES accounts (id) ON DELETE SET NULL,
    CONSTRAINT invites_token_hash_key UNIQUE (token_hash),
    CONSTRAINT invites_kind_check CHECK (kind IN ('invite', 'setup')),
    CONSTRAINT invites_token_hash_check CHECK (octet_length(token_hash) = 32),
    CONSTRAINT invites_grants_role_check CHECK (grants_role IN ('user', 'admin')),
    CONSTRAINT invites_setup_role_check CHECK (kind <> 'setup' OR grants_role = 'admin'),
    CONSTRAINT invites_target_account_id_check CHECK (target_account_id IS NULL OR kind = 'invite'),
    CONSTRAINT invites_created_by_check CHECK (kind = 'invite' OR created_by IS NULL)
);
CREATE UNIQUE INDEX invites_open_setup_key ON invites ((true)) WHERE kind = 'setup' AND redeemed_at IS NULL;

CREATE TABLE oauth_states (
    state_hash bytea NOT NULL,
    provider text NOT NULL,
    nonce text NOT NULL,
    pkce_verifier text NOT NULL,
    return_to text NOT NULL,
    client_kind text NOT NULL,
    invite_token_hash bytea,
    code_challenge text,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    CONSTRAINT oauth_states_pkey PRIMARY KEY (state_hash),
    CONSTRAINT oauth_states_state_hash_check CHECK (octet_length(state_hash) = 32),
    CONSTRAINT oauth_states_invite_token_hash_check CHECK (octet_length(invite_token_hash) = 32),
    CONSTRAINT oauth_states_client_kind_check CHECK (client_kind IN ('web', 'android', 'ios')),
    CONSTRAINT oauth_states_return_to_check CHECK (return_to LIKE '/%' AND return_to NOT LIKE '//%')
);

CREATE TABLE login_codes (
    code_hash bytea NOT NULL,
    account_id uuid NOT NULL,
    code_challenge text NOT NULL,
    client_kind text NOT NULL,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    CONSTRAINT login_codes_pkey PRIMARY KEY (code_hash),
    CONSTRAINT login_codes_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT login_codes_code_hash_check CHECK (octet_length(code_hash) = 32),
    CONSTRAINT login_codes_client_kind_check CHECK (client_kind IN ('web', 'android', 'ios'))
);

ALTER TABLE identities ENABLE ROW LEVEL SECURITY;
ALTER TABLE sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE device_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE device_grants ENABLE ROW LEVEL SECURITY;

CREATE POLICY identities_tenant ON identities USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);
CREATE POLICY sessions_tenant ON sessions USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);
CREATE POLICY device_tokens_tenant ON device_tokens USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);
CREATE POLICY device_grants_tenant ON device_grants USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);

GRANT SELECT, INSERT, UPDATE, DELETE ON identities, sessions, device_tokens, device_grants, invites, oauth_states, login_codes TO sunnycloud;
