//! Component test harness on a real PostgreSQL 18: a clone of the migrated template of `sc_testdb`. The component
//! connects as `sunnycloud`; fixtures and probes run as `sunnycloud_migrator`. Time comes from a `ManualClock`,
//! the grant key is a fixed test key, and every test gets an `AbuseLimiter` of its own. The owner service reads
//! the real tree (02's `PgTreeReader`) and writes through the real `AuditWriter`; link passwords use a small
//! Argon2id cost set through the configuration key, never a lowered default.

use std::sync::Arc;
use std::time::Duration;

use sc_audit::AuditWriter;
use sc_clock::{Clock, ManualClock};
use sc_config::Environment;
use sc_db::Db;
use sc_ratelimit::{AbuseLimiter, LimitId, LimitSpec, LimitTable, Subject, Verdict};
pub use sc_testdb::{API, MIGRATOR, START, TestDb};
use sqlx::PgConnection;
use sunnycloud_sharing::{PgShareService, ShareConfig};
use sunnycloud_tree::{PgTreeReader, TreeConfig};
use uuid::Uuid;

use crate::settings::{KeyFile, TEST_GRANT_KEY, load_with_key};

/// Millisecond values, never the production ones; the `test` environment does not bind the duration floors.
const SHORT_DURATIONS: [(&str, &str); 5] = [
    ("SUNNYCLOUD_SHARE_LOOKUP_CACHE", "200ms"),
    ("SUNNYCLOUD_SHARE_GRANT_TTL", "500ms"),
    ("SUNNYCLOUD_SHARE_WRONG_PASSWORD_WINDOW", "500ms"),
    ("SUNNYCLOUD_SHARE_DEFAULT_TTL", "2s"),
    ("SUNNYCLOUD_SHARE_ARGON2_PARAMS", "64,1,1"),
];
/// The PHC prefix of a password hashed with the harness cost above.
pub const TEST_PHC_PREFIX: &str = "$argon2id$v=19$m=64,t=1,p=1$";
const INVALID_TOKEN_WINDOW: Duration = Duration::from_secs(60);

pub struct Harness {
    pub db: TestDb,
    pub api: Db,
    pub clock: Arc<ManualClock>,
    pub config: ShareConfig,
    pub tree_config: TreeConfig,
    pub limiter: AbuseLimiter,
}

impl Harness {
    pub async fn new() -> Harness {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let clock = sc_testdb::clock_at_start();
        let (_, loaded) = load_with_key(
            KeyFile::Bytes(TEST_GRANT_KEY.as_bytes()),
            Environment::Test,
            &SHORT_DURATIONS,
        );
        let settings =
            loaded.unwrap_or_else(|error| panic!("the harness settings do not load: {error}"));
        let config = ShareConfig::from_settings(&settings);
        let tree_config = TreeConfig::from_settings(&settings);
        let limiter = limiter(&config, clock.clone());
        Harness {
            db,
            api,
            clock,
            config,
            tree_config,
            limiter,
        }
    }

    /// The owner service with the harness section.
    pub fn shares(&self) -> PgShareService {
        self.shares_with(&self.config)
    }

    /// The owner service with `config`, over the real tree reader and audit writer on the API role.
    pub fn shares_with(&self, config: &ShareConfig) -> PgShareService {
        let tree = Arc::new(PgTreeReader::new(self.api.clone(), &self.tree_config));
        let audit = AuditWriter::new(self.api.clone(), self.clock.clone());
        PgShareService::new(self.api.clone(), self.clock.clone(), tree, audit, config)
            .expect("the harness sharing section is valid")
    }

    /// Fixtures and probes run as the schema owner.
    pub async fn fixture_conn(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// Drops the pool and the database; a connection still open afterwards is a leak of the test.
    pub async fn finish(self) {
        let Harness { db, api, .. } = self;
        drop(api);
        assert_eq!(db.finish().await, 0, "connections left open");
    }
}

fn limiter(config: &ShareConfig, clock: Arc<dyn Clock>) -> AbuseLimiter {
    let capacity = |limit: u64| u32::try_from(limit).expect("limit fits u32");
    let placeholder = LimitSpec::new(1, INVALID_TOKEN_WINDOW);
    let table = LimitTable::new(placeholder, placeholder)
        .with_spec(
            LimitId::SharePasswordLink,
            LimitSpec::new(
                capacity(config.wrong_password_limit),
                config.wrong_password_window,
            ),
        )
        .with_spec(
            LimitId::ShareTokenIp,
            LimitSpec::new(capacity(config.invalid_token_limit), INVALID_TOKEN_WINDOW),
        );
    AbuseLimiter::new(clock, table).expect("limit table")
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

/// An account with its root folder and one file under it; returns the file id.
pub async fn account_with_file(conn: &mut PgConnection, id: Uuid) -> Uuid {
    account(conn, id).await;
    let (root, file) = (Uuid::new_v4(), Uuid::new_v4());
    for (node, parent, kind, name) in [
        (root, None, "folder", ""),
        (file, Some(root), "file", "f.txt"),
    ] {
        sqlx::query(
            "INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $5, 1, 'epoch'::timestamptz, 'epoch'::timestamptz)",
        )
        .bind(node)
        .bind(id)
        .bind(parent)
        .bind(kind)
        .bind(name)
        .execute(&mut *conn)
        .await
        .expect("insert node");
    }
    file
}

/// A link with a token hash made of `seed`; returns the link id.
pub async fn link(
    conn: &mut PgConnection,
    account: Uuid,
    node: Uuid,
    seed: u8,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO share_links (id, account_id, node_id, token_hash, expires_at, created_at) \
         VALUES ($1, $2, $3, $4, 'epoch'::timestamptz + interval '7 days', 'epoch'::timestamptz)",
    )
    .bind(id)
    .bind(account)
    .bind(node)
    .bind([seed; 32].as_slice())
    .execute(conn)
    .await
    .map(|_| id)
}

pub fn sqlstate(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .map(|code| code.into_owned())
        .unwrap_or_else(|| format!("not a database error: {error}"))
}

#[tokio::test]
async fn each_harness_has_its_own_limiter_driven_by_its_own_manual_clock() {
    // Arrange
    let first = Harness::new().await;
    let second = Harness::new().await;
    let link_id = Subject::Link(Uuid::new_v4());
    let limit = first.config.wrong_password_limit;

    // Act
    let verdicts: Vec<Verdict> = (0..limit)
        .map(|_| {
            first
                .limiter
                .record_failure(LimitId::SharePasswordLink, &link_id)
        })
        .collect();
    let limited = first.limiter.check(LimitId::SharePasswordLink, &link_id);
    let other = second.limiter.check(LimitId::SharePasswordLink, &link_id);
    first.clock.tick(first.config.wrong_password_window);
    let refilled = first.limiter.check(LimitId::SharePasswordLink, &link_id);

    // Assert
    assert!(matches!(verdicts.last(), Some(Verdict::Tripped { .. })));
    assert!(matches!(limited, Verdict::Limited { .. }));
    assert_eq!(other, Verdict::Allowed);
    assert_eq!(refilled, Verdict::Allowed);
    first.finish().await;
    second.finish().await;
}
