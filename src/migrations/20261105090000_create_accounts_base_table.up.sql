-- Owner: shared/db (domain word accounts_base). The minimal accounts table every tenant table references;
-- the identity epic adds its columns in its own migration. Ids and timestamps come from the application, so
-- neither column has a default. No row-level security: accounts is keyed by id, not by a tenant column.
CREATE TABLE accounts (
    id uuid NOT NULL,
    state text NOT NULL,
    created_at timestamptz NOT NULL,
    CONSTRAINT accounts_pkey PRIMARY KEY (id),
    CONSTRAINT accounts_state_check CHECK (state IN ('active', 'disabled', 'deleting'))
);

GRANT SELECT, INSERT, UPDATE, DELETE ON accounts TO sunnycloud;

-- The API role reads the applied versions to refuse a schema that does not match its binary.
GRANT SELECT ON _sqlx_migrations TO sunnycloud;
