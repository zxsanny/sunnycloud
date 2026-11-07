//! One database per test, a clone of the migrated `sc_testdb` template. The component side connects as
//! `sunnycloud`, fixtures and probes as `sunnycloud_migrator`.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use sc_config::{ApiSettings, ConfigError, ConfigSources, Environment, TABLE, load};
use sc_db::Db;
pub use sc_testdb::{API, MIGRATOR, START, TestDb};
use sqlx::PgConnection;
use uuid::Uuid;

pub struct Fixture {
    pub db: TestDb,
    pub api: Db,
}

impl Fixture {
    pub async fn new() -> Fixture {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        Fixture { db, api }
    }

    pub async fn owner_conn(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// Drops the pool and the database; a connection still open afterwards is a leak of the test.
    pub async fn finish(self) {
        let Fixture { db, api } = self;
        drop(api);
        assert_eq!(db.finish().await, 0, "connections left open");
    }
}

pub async fn account(conn: &mut PgConnection, id: Uuid) {
    sqlx::query(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ($1, 'active', 'epoch'::timestamptz + $2::bigint * interval '1 microsecond', 'Fixture', 'user')",
    )
    .bind(id)
    .bind(START.as_unix_micros())
    .execute(conn)
    .await
    .expect("insert account");
}

pub async fn blob(conn: &mut PgConnection, id: Uuid, account: Uuid) {
    sqlx::query(
        "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
         VALUES ($1, $2, 'committed', 1, '\\x00', 1, 'epoch'::timestamptz)",
    )
    .bind(id)
    .bind(account)
    .execute(conn)
    .await
    .expect("insert blob");
}

/// An account with a root folder, a file under it and one revision of that file over `content_hash`; returns the
/// revision id.
pub async fn account_with_revision(
    conn: &mut PgConnection,
    account_id: Uuid,
    content_hash: &[u8],
) -> Uuid {
    account(conn, account_id).await;
    let (root, file, blob_id, revision) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    blob(conn, blob_id, account_id).await;
    for (id, parent, kind, name) in [
        (root, None, "folder", ""),
        (file, Some(root), "file", "a.jpg"),
    ] {
        sqlx::query(
            "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $5, 1, 'epoch'::timestamptz, 'epoch'::timestamptz)",
        )
        .bind(id)
        .bind(account_id)
        .bind(parent)
        .bind(kind)
        .bind(name)
        .execute(&mut *conn)
        .await
        .expect("insert node");
    }
    sqlx::query(
        "INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, blocklist, created_at) \
         VALUES ($1, $2, $3, $4, 1, $5, '\\x', 'epoch'::timestamptz)",
    )
    .bind(revision)
    .bind(account_id)
    .bind(file)
    .bind(blob_id)
    .bind(content_hash)
    .execute(conn)
    .await
    .expect("insert revision");
    revision
}

pub struct JobRow<'a> {
    pub account: Uuid,
    pub revision: Uuid,
    pub kind: &'a str,
    pub content_hash: &'a [u8],
    pub params_version: i16,
}

impl<'a> JobRow<'a> {
    pub fn new(account: Uuid, revision: Uuid, content_hash: &'a [u8]) -> Self {
        JobRow {
            account,
            revision,
            kind: "stills",
            content_hash,
            params_version: 1,
        }
    }
}

/// A queued job; returns its id.
pub async fn job(conn: &mut PgConnection, row: &JobRow<'_>) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO jobs (id, account_id, revision_id, kind, content_hash, params_version, priority, status, available_at, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, 0, 'queued', 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(id)
    .bind(row.account)
    .bind(row.revision)
    .bind(row.kind)
    .bind(row.content_hash)
    .bind(row.params_version)
    .execute(conn)
    .await
    .map(|_| id)
}

pub struct DerivedRow<'a> {
    pub account: Uuid,
    pub content_hash: &'a [u8],
    pub kind: &'a str,
    pub params_version: i16,
    pub blob: Uuid,
}

pub async fn derived(conn: &mut PgConnection, row: &DerivedRow<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO derived (account_id, content_hash, kind, params_version, blob_id, media_type, size, created_at) \
         VALUES ($1, $2, $3, $4, $5, 'image/webp', 10, 'epoch'::timestamptz)",
    )
    .bind(row.account)
    .bind(row.content_hash)
    .bind(row.kind)
    .bind(row.params_version)
    .bind(row.blob)
    .execute(conn)
    .await
    .map(drop)
}

pub async fn worker_status(conn: &mut PgConnection, worker_id: &str) {
    sqlx::query(
        "INSERT INTO worker_status (worker_id, seen_at, image_age_days, decoder_ok) \
         VALUES ($1, 'epoch'::timestamptz, 3, true)",
    )
    .bind(worker_id)
    .execute(conn)
    .await
    .expect("insert worker status");
}

pub fn sqlstate(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned())
        .unwrap_or_else(|| format!("not a database error: {error}"))
}

pub fn constraint(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|database| database.constraint())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("no constraint: {error}"))
}

/// A secrets folder holding a file for every secret the API section needs; `tokens` replaces the contents of
/// the worker token files.
fn write_secrets(path: &PathBuf, tokens: &[(&str, &str)]) {
    fs::create_dir_all(path).expect("create secrets dir");
    let needed = TABLE.iter().filter(|spec| {
        spec.secret
            && (spec.key == "provider_github_client_secret"
                || (spec.default.is_none() && !spec.optional))
    });
    for spec in needed {
        fs::write(path.join(spec.key), "sample-secret\n").expect("write secret file");
    }
    for (key, value) in tokens {
        fs::write(path.join(key), format!("{value}\n")).expect("write token file");
    }
}

fn shared_secrets_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("sc_media_secrets_{}", std::process::id()));
        write_secrets(&path, &[]);
        path
    })
    .clone()
}

fn load_from(
    secrets_dir: PathBuf,
    extra: &[(&str, &str)],
) -> Result<Arc<ApiSettings>, ConfigError> {
    let mut sources = ConfigSources {
        secrets_dir,
        environment: Environment::Production,
        ..ConfigSources::default()
    };
    let required = [
        ("SUNNYCLOUD_PUBLIC_URL", "https://sunnycloud.test"),
        ("SUNNYCLOUD_GITHUB_CLIENT_ID", "gh-client"),
        ("SUNNYCLOUD_BLOB_ROOT", "/var/lib/sunnycloud/blobs"),
    ];
    for (key, value) in required.iter().chain(extra) {
        sources.env.insert((*key).to_owned(), (*value).to_owned());
    }
    load::<ApiSettings>(&sources)
}

/// An API-section load with every required key present, plus `extra` variables. Duration ranges bind only in
/// production, so that is the environment of the load.
pub fn load_settings(extra: &[(&str, &str)]) -> Result<Arc<ApiSettings>, ConfigError> {
    load_from(shared_secrets_dir(), extra)
}

/// The same load from a secrets folder of its own whose worker token files hold `tokens`; the folder is removed
/// before the settings are returned, so the values can only have been read once, at load.
pub fn load_settings_with_tokens(tokens: &[(&str, &str)]) -> Result<Arc<ApiSettings>, ConfigError> {
    let path = std::env::temp_dir().join(format!("sc_media_tokens_{}", Uuid::new_v4().simple()));
    write_secrets(&path, tokens);
    let loaded = load_from(path.clone(), &[]);
    fs::remove_dir_all(&path).expect("remove the secrets dir");
    loaded
}
