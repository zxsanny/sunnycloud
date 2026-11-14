//! AZ-3822 AC-1, AC-2, AC-3 (IT-07-14): in development the seed creates its accounts through the real sign-up
//! path (setup token and invites redeemed by sign-ins at the identity provider) and its content through the
//! real service ports, and the photo commits queue media jobs through 08's shared effect; a second run adds
//! nothing; production is refused with nothing written. AZ-4311 AC-1 end to end: the one effect queues each
//! user's photo under that user's account.

#[allow(dead_code)]
#[path = "../../../01_identity/tests/support/provider.rs"]
mod provider;
#[path = "../support/mod.rs"]
mod support;
mod world;

use contracts::{DevSeed, DevSeedError};
use sc_config::Environment;
use sqlx::PgConnection;
use world::SeedWorld;

const GIB: i64 = 1 << 30;

/// Row counts of what the seed creates: accounts, admins that are active, users, nodes, share links, open
/// upload sessions, accounts with a 1 GiB limit, and media jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Installation {
    accounts: i64,
    active_admins: i64,
    users: i64,
    nodes: i64,
    shares: i64,
    open_uploads: i64,
    gib_limits: i64,
    jobs: i64,
}

async fn installation(conn: &mut PgConnection) -> Installation {
    let row: (i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM accounts), \
         (SELECT count(*) FROM accounts WHERE role = 'admin' AND state = 'active'), \
         (SELECT count(*) FROM accounts WHERE role = 'user'), \
         (SELECT count(*) FROM nodes), \
         (SELECT count(*) FROM share_links), \
         (SELECT count(*) FROM upload_sessions), \
         (SELECT count(*) FROM quota WHERE limit_bytes = $1), \
         (SELECT count(*) FROM jobs)",
    )
    .bind(GIB)
    .fetch_one(conn)
    .await
    .expect("count the installation");
    Installation {
        accounts: row.0,
        active_admins: row.1,
        users: row.2,
        nodes: row.3,
        shares: row.4,
        open_uploads: row.5,
        gib_limits: row.6,
        jobs: row.7,
    }
}

/// Users that own exactly one share link.
async fn users_with_one_share(conn: &mut PgConnection) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM accounts a WHERE a.role = 'user' \
         AND (SELECT count(*) FROM share_links s WHERE s.account_id = a.id) = 1",
    )
    .fetch_one(conn)
    .await
    .expect("count users with a share")
}

/// Users with exactly one queued stills job, under their own account and for a revision of their own.
async fn users_with_one_own_photo_job(conn: &mut PgConnection) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM accounts a WHERE a.role = 'user' \
         AND (SELECT count(*) FROM jobs j JOIN revisions r ON r.id = j.revision_id \
              WHERE j.account_id = a.id AND r.account_id = a.id \
              AND j.status = 'queued' AND j.kind = 'stills') = 1",
    )
    .fetch_one(conn)
    .await
    .expect("count users with a photo job")
}

/// Folder names of the seeded users that are not plain ASCII or mix upper and lower case.
async fn unicode_and_mixed_case_folders(conn: &mut PgConnection) -> (i64, i64) {
    sqlx::query_as(
        "SELECT count(*) FILTER (WHERE name !~ '^[ -~]*$'), \
         count(*) FILTER (WHERE name ~ '[A-Z]' AND name ~ '[a-z]') \
         FROM nodes WHERE kind = 'folder' AND name <> ''",
    )
    .fetch_one(conn)
    .await
    .expect("read folder names")
}

#[tokio::test]
async fn in_development_the_seed_creates_the_demo_installation_through_the_ports() {
    // Arrange
    let world = SeedWorld::new(Environment::Development).await;

    // Act
    let result = world.seed.seed().await;

    // Assert
    let mut conn = world.fixture().await;
    assert!(result.is_ok(), "{result:?}");
    let seeded = installation(&mut conn).await;
    assert_eq!(
        (seeded.accounts, seeded.active_admins, seeded.users),
        (3, 1, 2)
    );
    assert_eq!(seeded.gib_limits, 1);
    assert_eq!(users_with_one_share(&mut conn).await, 2);
    assert_eq!(seeded.open_uploads, 1);
    assert_eq!(seeded.jobs, 2, "one media job per committed photo");
    assert_eq!(users_with_one_own_photo_job(&mut conn).await, 2);
    let (unicode, mixed_case) = unicode_and_mixed_case_folders(&mut conn).await;
    assert!(unicode >= 2 && mixed_case >= 2, "{unicode} {mixed_case}");
    assert_eq!(
        world.idp.hits("/token"),
        3,
        "one provider sign-in per account"
    );
    drop(conn);
    world.finish().await;
}

#[tokio::test]
async fn a_second_seed_adds_no_account_node_or_share() {
    // Arrange
    let world = SeedWorld::new(Environment::Development).await;
    world.seed.seed().await.expect("the first seed");
    let mut conn = world.fixture().await;
    let first = installation(&mut conn).await;

    // Act
    let result = world.seed.seed().await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(installation(&mut conn).await, first);
    drop(conn);
    world.finish().await;
}

#[tokio::test]
async fn in_production_the_seed_is_refused_and_writes_nothing() {
    // Arrange
    let world = SeedWorld::new(Environment::Production).await;

    // Act
    let result = world.seed.seed().await;

    // Assert
    let mut conn = world.fixture().await;
    assert!(matches!(result, Err(DevSeedError::RefusedInProduction)));
    let counts = installation(&mut conn).await;
    assert_eq!((counts.accounts, counts.nodes), (0, 0));
    assert_eq!(world.idp.hits(""), 0, "no provider call");
    drop(conn);
    world.finish().await;
}
