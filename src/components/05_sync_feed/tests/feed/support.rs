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

pub async fn feed_row(
    conn: &mut PgConnection,
    account: Uuid,
    epoch: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO account_feeds (account_id, epoch) VALUES ($1, $2)")
        .bind(account)
        .bind(epoch)
        .execute(conn)
        .await
        .map(drop)
}

/// An account with its feed row.
pub async fn account_with_feed(conn: &mut PgConnection, id: Uuid, epoch: i64) {
    account(conn, id).await;
    feed_row(conn, id, epoch).await.expect("insert feed row");
}

/// The root folder of the account, as the tree creates it.
pub async fn root_node(conn: &mut PgConnection, account: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO nodes (id, account_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         VALUES ($1, $2, 'folder', '', '', 1, 'epoch'::timestamptz, 'epoch'::timestamptz)",
    )
    .bind(id)
    .bind(account)
    .execute(conn)
    .await
    .expect("insert root node");
    id
}

pub fn sqlstate(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned())
        .unwrap_or_else(|| format!("not a database error: {error}"))
}

/// One secrets folder per test process, filled once with a file for every secret the API section needs.
fn secrets_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("sc_feed_secrets_{}", std::process::id()));
        fs::create_dir_all(&path).expect("create secrets dir");
        let needed = TABLE.iter().filter(|spec| {
            spec.secret
                && (spec.key == "provider_github_client_secret"
                    || (spec.default.is_none() && !spec.optional))
        });
        for spec in needed {
            fs::write(path.join(spec.key), "sample-secret\n").expect("write secret file");
        }
        path
    })
    .clone()
}

/// An API-section load with every required key present, plus `extra` variables. Duration ranges bind only in
/// production, so that is the environment of the load.
pub fn load_settings(extra: &[(&str, &str)]) -> Result<Arc<ApiSettings>, ConfigError> {
    let mut sources = ConfigSources {
        secrets_dir: secrets_dir(),
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
