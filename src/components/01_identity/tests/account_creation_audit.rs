//! The mandatory `account created` audit event (owner decision 2026-11-13, AZ-3691): written as the last
//! statement of the account-creation transaction with ids and codes only, and a failed write rolls the whole
//! creation back so the invite stays redeemable. Real hooks of 02, 05 and 07a, PostgreSQL 18, manual clock.

mod support;

use contracts::{SignInError, SignInOutcome};
use support::registration::{INVITE, Rig, Rows, sign_in};
use support::sign_in::OIDC;
use support::{START, audit};

const POOL: u32 = 5;

#[tokio::test]
async fn a_created_account_writes_one_account_created_event_with_ids_and_the_granted_role() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let invite = rig
        .invite(INVITE, "admin", START.as_unix_micros() + 60_000_000)
        .await;

    // Act
    let outcome = sign_in(&rig.world, OIDC, "new-sub", Some(INVITE), |_| {}).await;

    // Assert
    assert!(
        matches!(outcome, Ok(SignInOutcome::Session { .. })),
        "{outcome:?}"
    );
    let (account, _, _) = rig
        .account_of(OIDC, "new-sub")
        .await
        .expect("the account exists");
    let identity: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM identities WHERE account_id = $1")
            .bind(account)
            .fetch_one(&mut rig.world.conn)
            .await
            .expect("read the identity");
    let rows = audit::of(&mut rig.world.conn, "sign_in").await;
    let details: Vec<Option<String>> = rows.iter().map(|row| row.detail.clone()).collect();
    assert_eq!(
        details,
        vec![
            Some(format!(
                r#"{{"role": "admin", "action": "account_created", "invite_id": "{invite}", "identity_id": "{identity}"}}"#
            )),
            Some(r#"{"action": "session_started"}"#.to_owned()),
        ]
    );
    assert_eq!(rows[0].actor_account_id, Some(account));
    assert_eq!(rows[0].subject_id, Some(account));
    rig.finish().await;
}

#[tokio::test]
async fn a_failed_account_created_write_rolls_the_creation_back_and_the_invite_stays_open() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    rig.member_invite(INVITE).await;
    audit::block_inserts(&mut rig.world.conn).await;

    // Act
    let outcome = sign_in(&rig.world, OIDC, "new-sub", Some(INVITE), |_| {}).await;

    // Assert
    assert!(
        matches!(outcome, Err(SignInError::Unexpected(_))),
        "{outcome:?}"
    );
    assert_eq!(
        rig.rows().await,
        Rows {
            accounts: 0,
            identities: 0,
            root_nodes: 0,
            feeds: 0,
            quotas: 0,
            redeemed_invites: 0,
        }
    );
    rig.finish().await;
}
