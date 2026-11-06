//! AC-3 and AC-5 of the account administration (IT-01-15; NFT-SEC-27): finalizing leaves nothing of the account,
//! and a role change shows in the listing with its state and last login. Real PostgreSQL, the manual clock.

mod support;

use contracts::{AccountAdmin, AccountFilter, AccountStatus, Page, Role};
use sc_db::{RunError, Scope};
use sqlx::PgConnection;
use support::accounts::{accounts, fixture, owned_rows, seed};
use support::{START, fixtures};
use uuid::Uuid;

/// One row of each of the six tables for `account`, a login code included.
async fn fill_six_tables(conn: &mut PgConnection, account: Uuid, tag: &str) {
    let at = START.as_unix_micros();
    fixtures::identity_at(conn, account, "oidc", &format!("{tag}-subject"), at).await;
    fixtures::device_grant(
        conn,
        Some(account),
        &format!("{tag}-grant"),
        &format!("{tag}-UC"),
        "approved",
        at,
    )
    .await
    .expect("grant");
    fixtures::login_code(conn, account, &format!("{tag}-code"), at).await;
}

#[tokio::test]
async fn finalize_leaves_no_row_of_the_account_in_any_of_the_six_tables_and_touches_no_other_account()
 {
    // Arrange
    let mut f = fixture().await;
    let account = seed(&mut f.conn, "a").await;
    let other = seed(&mut f.conn, "b").await;
    fill_six_tables(&mut f.conn, account, "a").await;
    fill_six_tables(&mut f.conn, other, "b").await;
    let before = owned_rows(&mut f.conn, account).await;
    let other_before = owned_rows(&mut f.conn, other).await;
    let admin = accounts(&f.harness);
    f.harness
        .api
        .run(Scope::Account(account), async |tx| {
            admin.begin_deletion(tx, account).await
        })
        .await
        .expect("begin");

    // Act
    f.harness
        .api
        .run(Scope::Account(account), async |tx| {
            admin.finalize_deletion(tx, account).await
        })
        .await
        .expect("finalize commits");

    // Assert
    assert_eq!(before, [1, 2, 2, 1, 1, 1]);
    assert_eq!(owned_rows(&mut f.conn, account).await, [0; 6]);
    assert_eq!(owned_rows(&mut f.conn, other).await, other_before);
    drop(admin);
    f.finish().await;
}

#[tokio::test]
async fn finalize_refuses_an_account_that_is_not_deleting_and_accepts_one_that_is_already_gone() {
    // Arrange
    let mut f = fixture().await;
    let account = seed(&mut f.conn, "a").await;
    let admin = accounts(&f.harness);
    let gone = Uuid::new_v4();

    // Act
    let on_active = f
        .harness
        .api
        .run(Scope::Account(account), async |tx| {
            admin.finalize_deletion(tx, account).await
        })
        .await;
    let on_gone = f
        .harness
        .api
        .run(Scope::Account(gone), async |tx| {
            admin.finalize_deletion(tx, gone).await
        })
        .await;

    // Assert
    assert!(
        matches!(on_active, Err(RunError::Closure(_))),
        "{on_active:?}"
    );
    assert!(on_gone.is_ok(), "{on_gone:?}");
    assert_eq!(owned_rows(&mut f.conn, account).await, [0, 2, 2, 0, 0, 1]);
    drop(admin);
    f.finish().await;
}

#[tokio::test]
async fn role_change_commits_and_the_listing_shows_role_state_and_last_login() {
    // Arrange
    let mut f = fixture().await;
    let member = Uuid::new_v4();
    let at = START.as_unix_micros();
    fixtures::account(&mut f.conn, member, at).await;
    fixtures::identity_at(&mut f.conn, member, "oidc", "member-subject", at).await;
    sqlx::query("UPDATE identities SET last_login_at = 'epoch'::timestamptz + $1::bigint * interval '1 microsecond'")
        .bind(at + 5_000_000)
        .execute(&mut f.conn)
        .await
        .expect("last login");
    let disabled = Uuid::new_v4();
    fixtures::account_in(&mut f.conn, disabled, "user", "disabled", at + 1).await;
    let admin = accounts(&f.harness);

    // Act
    f.harness
        .api
        .run(Scope::Account(member), async |tx| {
            admin.set_role(tx, member, Role::Admin).await
        })
        .await
        .expect("set_role commits");
    let admins = admin
        .list_accounts(
            AccountFilter {
                role: Some(Role::Admin),
                state: None,
            },
            Page {
                offset: 0,
                limit: 10,
            },
        )
        .await
        .expect("list admins");
    let everyone = admin
        .list_accounts(
            AccountFilter::default(),
            Page {
                offset: 0,
                limit: 10,
            },
        )
        .await
        .expect("list all");
    let disabled_users = admin
        .list_accounts(
            AccountFilter {
                role: None,
                state: Some(AccountStatus::Disabled),
            },
            Page {
                offset: 0,
                limit: 10,
            },
        )
        .await
        .expect("list disabled");
    let second_page = admin
        .list_accounts(
            AccountFilter::default(),
            Page {
                offset: 1,
                limit: 1,
            },
        )
        .await
        .expect("second page");

    // Assert
    assert_eq!(admins.len(), 1);
    let listed = &admins[0];
    assert_eq!(
        (listed.id, listed.role, listed.state),
        (member, Role::Admin, AccountStatus::Active)
    );
    assert_eq!(listed.display_name, "Fixture");
    assert_eq!(
        listed.last_login.map(|login| login.as_unix_micros()),
        Some(at + 5_000_000)
    );
    assert_eq!(
        everyone.iter().map(|info| info.id).collect::<Vec<_>>(),
        [member, disabled]
    );
    assert_eq!(everyone[1].last_login, None);
    assert_eq!(
        disabled_users
            .iter()
            .map(|info| info.id)
            .collect::<Vec<_>>(),
        [disabled]
    );
    assert_eq!(
        second_page.iter().map(|info| info.id).collect::<Vec<_>>(),
        [disabled]
    );
    drop(admin);
    f.finish().await;
}

#[tokio::test]
async fn set_role_of_an_unknown_account_is_an_error() {
    // Arrange
    let f = fixture().await;
    let admin = accounts(&f.harness);
    let unknown = Uuid::new_v4();

    // Act
    let result = f
        .harness
        .api
        .run(Scope::Account(unknown), async |tx| {
            admin.set_role(tx, unknown, Role::Admin).await
        })
        .await;

    // Assert
    assert!(matches!(result, Err(RunError::Closure(_))), "{result:?}");
    drop(admin);
    f.finish().await;
}
