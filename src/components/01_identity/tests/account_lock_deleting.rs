//! `AccountAdmin::lock_deleting` (AZ-3820, CC-1): the purger's per-step claim on a `deleting` account row,
//! `FOR NO KEY UPDATE SKIP LOCKED`. Only a `deleting` row is claimed; a second claimer gets `false` at once; while
//! the claim is held, an insert that references the account on another connection does not wait. The listing
//! carries `deletion_requested_at`. Real PostgreSQL, the manual clock.

mod support;

use std::time::Duration;

use contracts::{AccountAdmin, AccountFilter, AccountStatus, Page};
use sc_db::{DbError, Scope};
use support::START;
use support::accounts::{accounts, fixture, seed};
use uuid::Uuid;

/// Far longer than an insert takes; reached only when the insert waits for the claim.
const INSERT_WAIT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn lock_deleting_claims_only_a_deleting_account() {
    // Arrange
    let mut f = fixture().await;
    let admin = accounts(&f.harness);
    let active = seed(&mut f.conn, "a").await;
    let deleting = seed(&mut f.conn, "d").await;
    let unknown = Uuid::new_v4();
    f.harness
        .api
        .run(Scope::Account(deleting), async |tx| {
            admin.begin_deletion(tx, deleting).await
        })
        .await
        .expect("begin");

    // Act
    let mut claims = Vec::new();
    for account in [active, deleting, unknown] {
        let claimed = f
            .harness
            .api
            .run(Scope::Account(account), async |tx| {
                admin.lock_deleting(tx, account).await
            })
            .await
            .expect("lock_deleting");
        claims.push(claimed);
    }

    // Assert
    assert_eq!(claims, [false, true, false]);
    drop(admin);
    f.finish().await;
}

#[tokio::test]
async fn a_second_claim_is_skipped_and_an_insert_referencing_the_account_does_not_wait() {
    // Arrange
    let mut f = fixture().await;
    let admin = accounts(&f.harness);
    let account = seed(&mut f.conn, "d").await;
    f.harness
        .api
        .run(Scope::Account(account), async |tx| {
            admin.begin_deletion(tx, account).await
        })
        .await
        .expect("begin");
    let api = f.harness.api.clone();

    // Act
    let (first, second, inserted) = api
        .run(Scope::Account(account), async |tx| {
            let first = admin.lock_deleting(tx, account).await.expect("first claim");
            let second = api
                .run(Scope::Account(account), async |other| {
                    admin.lock_deleting(other, account).await
                })
                .await
                .expect("second claim");
            let insert = api.run(Scope::Account(account), async |other| {
                sqlx::query(
                    "INSERT INTO login_codes \
                     (code_hash, account_id, code_challenge, client_kind, created_at, expires_at) \
                     VALUES (sha256(gen_random_uuid()::text::bytea), $1, 'challenge', 'web', \
                     timestamptz '2026-11-07 12:00:00+00', timestamptz '2026-11-07 13:00:00+00')",
                )
                .bind(account)
                .execute(other.conn())
                .await
                .map(drop)
                .map_err(DbError::from)
            });
            let inserted = tokio::time::timeout(INSERT_WAIT, insert).await;
            Ok::<_, DbError>((first, second, matches!(inserted, Ok(Ok(())))))
        })
        .await
        .expect("claim transaction");

    // Assert
    assert!(first);
    assert!(!second, "the held row is skipped, not waited for");
    assert!(inserted, "the insert waited for the claim");
    drop((admin, api));
    f.finish().await;
}

#[tokio::test]
async fn the_listing_carries_the_deletion_request_time_of_a_deleting_account_only() {
    // Arrange
    let mut f = fixture().await;
    let admin = accounts(&f.harness);
    let active = seed(&mut f.conn, "a").await;
    let deleting = seed(&mut f.conn, "d").await;
    f.harness.clock.tick(Duration::from_secs(7));
    f.harness
        .api
        .run(Scope::Account(deleting), async |tx| {
            admin.begin_deletion(tx, deleting).await
        })
        .await
        .expect("begin");

    // Act
    let listed = admin
        .list_accounts(
            AccountFilter::default(),
            Page {
                offset: 0,
                limit: 10,
            },
        )
        .await
        .expect("list");

    // Assert
    let requested = |id: Uuid| {
        listed
            .iter()
            .find(|info| info.id == id)
            .map(|info| (info.state, info.deletion_requested_at))
    };
    assert_eq!(requested(active), Some((AccountStatus::Active, None)));
    assert_eq!(
        requested(deleting).map(|(state, at)| (state, at.map(|at| at.as_unix_micros()))),
        Some((
            AccountStatus::Deleting,
            Some(START.as_unix_micros() + 7_000_000)
        ))
    );
    drop(admin);
    f.finish().await;
}
