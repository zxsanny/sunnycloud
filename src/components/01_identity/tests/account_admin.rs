//! AC-1 and AC-2 of the account administration (IT-01-15; FT-P-19, NFT-SEC-08): the operations run in the
//! caller's transaction, so its rollback undoes them; disabling and deletion end access. Real PostgreSQL, the
//! manual clock.

mod support;

use std::time::Duration;

use contracts::{
    AccountAdmin, AccountAdminError, AccountGuard, AccountGuardError, AccountState, Authenticator,
    AuthenticatorError, DeviceGrantError, DeviceGrantService, PrincipalRevalidator,
};
use sc_clock::Clock;
use sc_db::{RunError, Scope};
use support::accounts::{
    accounts, device_token, fixture, revoke_reasons, revoked, seed, session_token, state_of,
};
use support::auth::{authenticator, device, session};
use support::device::{approve, geo, grants, ip, member};
use support::{Harness, START, fixtures};
use uuid::Uuid;

const POLL: Duration = Duration::from_millis(20);
const MARGIN: Duration = Duration::from_millis(50);

fn rollback() -> AccountAdminError {
    AccountAdminError::Unexpected("rolled back by the test".into())
}

/// `set_state(state)` in a caller transaction that commits.
async fn commit_set_state(harness: &Harness, account: Uuid, state: AccountState) {
    let admin = accounts(harness);
    harness
        .api
        .run(Scope::Account(account), async |tx| {
            admin.set_state(tx, account, state).await
        })
        .await
        .expect("set_state commits");
}

async fn all_dead(harness: &Harness, tag: &str) {
    let authenticator = authenticator(harness);
    for n in 1..=2 {
        let session = authenticator
            .authenticate(session(&session_token(tag, n)))
            .await;
        let device = authenticator
            .authenticate(device(&device_token(tag, n)))
            .await;
        assert!(
            matches!(session, Err(AuthenticatorError::Unauthenticated)),
            "{session:?}"
        );
        assert!(
            matches!(device, Err(AuthenticatorError::Unauthenticated)),
            "{device:?}"
        );
    }
}

#[tokio::test]
async fn disable_rolled_back_changes_nothing_and_committed_revokes_and_ends_access_within_the_cache_bound()
 {
    // Arrange
    let mut f = fixture().await;
    let account = seed(&mut f.conn, "a").await;
    let other = seed(&mut f.conn, "b").await;
    let admin = accounts(&f.harness);
    let ttl = f.harness.config.session_cache_ttl;
    let warm = authenticator(&f.harness);
    let principal = warm
        .authenticate(session(&session_token("a", 1)))
        .await
        .expect("live");
    assert!(warm.revalidate(&principal).await.expect("revalidate"));

    // Act
    let rolled_back = f
        .harness
        .api
        .run(Scope::Account(account), async |tx| {
            admin.set_state(tx, account, AccountState::Disabled).await?;
            Err::<(), _>(rollback())
        })
        .await;
    let after_rollback = (
        state_of(&mut f.conn, account).await,
        revoked(&mut f.conn, account).await,
    );
    commit_set_state(&f.harness, account, AccountState::Disabled).await;
    let mut waited = Duration::ZERO;
    while warm.revalidate(&principal).await.expect("revalidate") {
        f.harness.clock.tick(POLL);
        waited += POLL;
        assert!(waited <= ttl + MARGIN, "still valid after {waited:?}");
    }

    // Assert
    assert!(matches!(rolled_back, Err(RunError::Closure(_))));
    assert_eq!(after_rollback, ("active".to_owned(), (0, 0)));
    assert_eq!(state_of(&mut f.conn, account).await, "disabled");
    assert_eq!(revoked(&mut f.conn, account).await, (2, 2));
    assert_eq!(revoke_reasons(&mut f.conn, account).await, ["admin"]);
    all_dead(&f.harness, "a").await;
    assert_eq!(state_of(&mut f.conn, other).await, "active");
    assert_eq!(
        revoked(&mut f.conn, other).await,
        (0, 0),
        "another account is untouched"
    );
    drop((admin, warm));
    f.finish().await;
}

#[tokio::test]
async fn activating_a_disabled_account_does_not_bring_its_revoked_credentials_back() {
    // Arrange
    let mut f = fixture().await;
    let account = seed(&mut f.conn, "a").await;
    commit_set_state(&f.harness, account, AccountState::Disabled).await;

    // Act
    commit_set_state(&f.harness, account, AccountState::Active).await;

    // Assert
    assert_eq!(state_of(&mut f.conn, account).await, "active");
    assert_eq!(revoked(&mut f.conn, account).await, (2, 2));
    all_dead(&f.harness, "a").await;
    f.finish().await;
}

#[tokio::test]
async fn set_state_is_refused_for_a_deleting_account_and_for_an_unknown_one() {
    // Arrange
    let mut f = fixture().await;
    let account = seed(&mut f.conn, "a").await;
    fixtures::set_account_state(&mut f.conn, account, "deleting").await;
    let admin = accounts(&f.harness);
    let unknown = Uuid::new_v4();

    // Act
    let on_deleting = f
        .harness
        .api
        .run(Scope::Account(account), async |tx| {
            admin.set_state(tx, account, AccountState::Active).await
        })
        .await;
    let on_unknown = f
        .harness
        .api
        .run(Scope::Account(unknown), async |tx| {
            admin.set_state(tx, unknown, AccountState::Disabled).await
        })
        .await;

    // Assert
    assert!(
        matches!(on_deleting, Err(RunError::Closure(_))),
        "{on_deleting:?}"
    );
    assert!(
        matches!(on_unknown, Err(RunError::Closure(_))),
        "{on_unknown:?}"
    );
    assert_eq!(state_of(&mut f.conn, account).await, "deleting");
    drop(admin);
    f.finish().await;
}

#[tokio::test]
async fn begin_deletion_revokes_every_credential_and_grant_and_the_guard_refuses_at_once() {
    // Arrange
    let mut f = fixture().await;
    let geo = geo(&[]);
    let service = grants(&mut f.harness, geo);
    let approver = member(&mut f.conn).await;
    let account = approver.account_id;
    fixtures::live_session(
        &mut f.conn,
        account,
        "m-2",
        "phone",
        support::auth::FAR_FUTURE,
        START.as_unix_micros(),
    )
    .await;
    for n in 1..=2 {
        fixtures::named_device(
            &mut f.conn,
            account,
            &device_token("m", n),
            "laptop",
            START.as_unix_micros(),
        )
        .await;
    }
    let authorization = service
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    approve(
        &service,
        &approver,
        &authorization.user_code,
        ip("198.51.100.9"),
    )
    .await;
    let other = seed(&mut f.conn, "b").await;
    let admin = accounts(&f.harness);
    f.harness.clock.tick(Duration::from_secs(1));
    let requested_at = f.harness.clock.now_utc().as_unix_micros();

    // Act
    f.harness
        .api
        .run(Scope::Account(account), async |tx| {
            admin.begin_deletion(tx, account).await
        })
        .await
        .expect("begin_deletion commits");
    let guard = f
        .harness
        .api
        .run(Scope::Account(account), async |tx| {
            admin.ensure_active(tx, account).await
        })
        .await;
    let poll = service.poll(&authorization.device_code).await;

    // Assert
    let (state, stamped): (String, Option<i64>) = sqlx::query_as(
        "SELECT state, (extract(epoch FROM deletion_requested_at) * 1000000)::bigint FROM accounts WHERE id = $1",
    )
    .bind(account)
    .fetch_one(&mut f.conn)
    .await
    .expect("account row");
    assert_eq!((state.as_str(), stamped), ("deleting", Some(requested_at)));
    assert_eq!(revoked(&mut f.conn, account).await, (2, 2));
    assert_eq!(
        revoke_reasons(&mut f.conn, account).await,
        ["account_deleted"]
    );
    let statuses: Vec<String> =
        sqlx::query_scalar("SELECT status FROM device_grants WHERE account_id = $1")
            .bind(account)
            .fetch_all(&mut f.conn)
            .await
            .expect("grant statuses");
    assert_eq!(statuses, ["denied"]);
    assert!(
        matches!(poll, Err(DeviceGrantError::AccessDenied)),
        "{poll:?}"
    );
    let tokens: i64 =
        sqlx::query_scalar("SELECT count(*) FROM device_tokens WHERE account_id = $1")
            .bind(account)
            .fetch_one(&mut f.conn)
            .await
            .expect("count tokens");
    assert_eq!(tokens, 2, "the denied grant minted no token");
    assert!(
        matches!(
            guard,
            Err(RunError::Closure(AccountGuardError::AccountNotActive))
        ),
        "{guard:?}"
    );
    let authenticator = authenticator(&f.harness);
    for token in ["m-2", "m-d1", "m-d2", &format!("session-{account}")] {
        let result = if token.contains("-d") {
            authenticator.authenticate(device(token)).await
        } else {
            authenticator.authenticate(session(token)).await
        };
        assert!(
            matches!(result, Err(AuthenticatorError::Unauthenticated)),
            "{token}: {result:?}"
        );
    }
    assert_eq!(state_of(&mut f.conn, other).await, "active");
    assert_eq!(
        revoked(&mut f.conn, other).await,
        (0, 0),
        "another account is untouched"
    );
    drop((admin, service, authenticator));
    f.finish().await;
}

#[tokio::test]
async fn begin_deletion_rolled_back_changes_nothing_and_repeating_it_keeps_the_first_request_time()
{
    // Arrange
    let mut f = fixture().await;
    let account = seed(&mut f.conn, "a").await;
    let admin = accounts(&f.harness);

    // Act
    let rolled_back = f
        .harness
        .api
        .run(Scope::Account(account), async |tx| {
            admin.begin_deletion(tx, account).await?;
            Err::<(), _>(rollback())
        })
        .await;
    let unchanged = (
        state_of(&mut f.conn, account).await,
        revoked(&mut f.conn, account).await,
    );
    let first = f.harness.clock.now_utc().as_unix_micros();
    f.harness
        .api
        .run(Scope::Account(account), async |tx| {
            admin.begin_deletion(tx, account).await
        })
        .await
        .expect("first");
    f.harness.clock.tick(Duration::from_secs(60));
    f.harness
        .api
        .run(Scope::Account(account), async |tx| {
            admin.begin_deletion(tx, account).await
        })
        .await
        .expect("repeat");

    // Assert
    assert!(matches!(rolled_back, Err(RunError::Closure(_))));
    assert_eq!(unchanged, ("active".to_owned(), (0, 0)));
    let stamped: i64 = sqlx::query_scalar(
        "SELECT (extract(epoch FROM deletion_requested_at) * 1000000)::bigint FROM accounts WHERE id = $1",
    )
    .bind(account)
    .fetch_one(&mut f.conn)
    .await
    .expect("deletion_requested_at");
    assert_eq!(stamped, first);
    drop(admin);
    f.finish().await;
}
