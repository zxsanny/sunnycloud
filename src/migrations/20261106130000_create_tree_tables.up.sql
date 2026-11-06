-- Owner: components/02_tree (domain word tree). The two tables of the tree (data_model 5.1, 5.2, 12.3); `seq` and
-- `nodes_feed_idx` belong to 05 and arrive in its own migration. Ids and timestamps come from the application, so
-- no id or time column has a default. Every child row names its parent through (id, account_id), so a row can
-- never point at another account's row. Row-level security (ENABLE, no FORCE) covers both tables; the owner
-- (sunnycloud_migrator) bypasses it, the API role does not.
-- nodes and revisions reference each other, so the pointer from a node to its current revision is added once
-- both tables exist. Dropping a node while revisions of it remain is refused: the purge removes revisions first.
CREATE TABLE nodes (
    id uuid NOT NULL,
    account_id uuid NOT NULL,
    parent_id uuid,
    kind text NOT NULL,
    name text NOT NULL,
    name_key text COLLATE "C" NOT NULL,
    name_key_version smallint NOT NULL,
    current_revision_id uuid,
    deleted_at timestamptz,
    cascade_pending boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT nodes_pkey PRIMARY KEY (id),
    CONSTRAINT nodes_account_id_fkey FOREIGN KEY (account_id) REFERENCES accounts (id) ON DELETE RESTRICT,
    CONSTRAINT nodes_id_account_id_key UNIQUE (id, account_id),
    CONSTRAINT nodes_parent_id_account_id_fkey FOREIGN KEY (parent_id, account_id) REFERENCES nodes (id, account_id),
    CONSTRAINT nodes_kind_check CHECK (kind IN ('file', 'folder')),
    CONSTRAINT nodes_file_parent_check CHECK (kind = 'folder' OR parent_id IS NOT NULL),
    CONSTRAINT nodes_folder_revision_check CHECK (kind = 'file' OR current_revision_id IS NULL),
    CONSTRAINT nodes_name_check CHECK ((parent_id IS NULL) = (name = '') AND name NOT IN ('.', '..') AND position('/' IN name) = 0),
    CONSTRAINT nodes_name_size_check CHECK (octet_length(name) <= 255),
    CONSTRAINT nodes_name_key_size_check CHECK (octet_length(name_key) <= 1024)
);
CREATE UNIQUE INDEX nodes_parent_name_key ON nodes (parent_id, name_key) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX nodes_root_key ON nodes (account_id) WHERE parent_id IS NULL;
CREATE INDEX nodes_parent_idx ON nodes (parent_id);
CREATE INDEX nodes_tombstone_idx ON nodes (deleted_at) WHERE deleted_at IS NOT NULL;
CREATE INDEX nodes_cascade_idx ON nodes (account_id) WHERE cascade_pending;

-- The blocklist is stored out of line and the tuple threshold is low, so a listing join reads narrow rows.
CREATE TABLE revisions (
    id uuid NOT NULL,
    account_id uuid NOT NULL,
    node_id uuid NOT NULL,
    blob_id uuid NOT NULL,
    size bigint NOT NULL,
    content_hash bytea NOT NULL,
    blocklist bytea STORAGE EXTERNAL NOT NULL,
    media_type text,
    client_mtime timestamptz,
    client_meta jsonb,
    created_at timestamptz NOT NULL,
    CONSTRAINT revisions_pkey PRIMARY KEY (id),
    CONSTRAINT revisions_id_account_id_key UNIQUE (id, account_id),
    CONSTRAINT revisions_blob_id_key UNIQUE (blob_id),
    CONSTRAINT revisions_node_id_account_id_fkey FOREIGN KEY (node_id, account_id) REFERENCES nodes (id, account_id),
    CONSTRAINT revisions_blob_id_account_id_fkey FOREIGN KEY (blob_id, account_id) REFERENCES blobs (id, account_id) ON DELETE RESTRICT,
    CONSTRAINT revisions_size_check CHECK (size >= 0),
    CONSTRAINT revisions_content_hash_check CHECK (octet_length(content_hash) = 32),
    CONSTRAINT revisions_blocklist_check CHECK (octet_length(blocklist) % 32 = 0),
    CONSTRAINT revisions_client_meta_check CHECK (octet_length(client_meta::text) <= 4096)
) WITH (toast_tuple_target = 256);
CREATE INDEX revisions_node_id_created_at_idx ON revisions (node_id, created_at);
CREATE INDEX revisions_account_id_content_hash_idx ON revisions (account_id, content_hash);
CREATE INDEX revisions_client_device_idx ON revisions (account_id, (client_meta ->> 'device')) WHERE client_meta IS NOT NULL;

-- Deferred, so a file and its first revision can be inserted in either order inside one transaction. Removing
-- the revision row (retention purge, account erase) clears the pointer of a deleted file's tombstone.
ALTER TABLE nodes ADD CONSTRAINT nodes_current_revision_id_account_id_fkey
    FOREIGN KEY (current_revision_id, account_id) REFERENCES revisions (id, account_id)
    ON DELETE SET NULL (current_revision_id) DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE nodes ENABLE ROW LEVEL SECURITY;
ALTER TABLE revisions ENABLE ROW LEVEL SECURITY;

CREATE POLICY nodes_tenant ON nodes USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);
CREATE POLICY revisions_tenant ON revisions USING (
    account_id = (SELECT NULLIF(current_setting('sunnycloud.account_id', true), '')::uuid)
    OR current_setting('sunnycloud.scope', true) = 'system'
);

GRANT SELECT, INSERT, UPDATE, DELETE ON nodes, revisions TO sunnycloud;
