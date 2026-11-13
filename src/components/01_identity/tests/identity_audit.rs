//! AZ-3691 AC-5 and AC-6 (IT-01-10, NFT-SEC-38, ST-01-5) and the owner decisions of 2026-11-13 on mandatory
//! events: one `sign_in`, `device_approved`, `session_revoked` and `refresh_reuse` event with ids only and one WARN
//! line for the reuse; with every audit insert failing, a device approval and a sign-in keep their result and log
//! an ERROR, while a session revocation (mandatory) fails and revokes nothing. Real PostgreSQL, manual clock,
//! the in-process provider.

mod support;

use contracts::{
    Decision, DeviceGrantService, RevokeReason, SessionError, SessionService, SignInOutcome,
};
use sqlx::PgConnection;
use support::auth::FAR_FUTURE;
use support::device::{geo, grants, ip, member};
use support::logs::{Line, LogCapture};
use support::registration::{authorized, sign_in};
use support::sessions::{revocation, sessions};
use support::sign_in::{CLIENT_IP, HOST, OIDC, world};
use support::{START, audit, fixtures};
use tracing::Level;
use uuid::Uuid;

const SUBJECT: &str = "oidc-subject-1";
const EMAIL: &str = "alice@example.test";

async fn session_ids(conn: &mut PgConnection, account: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT id FROM sessions WHERE account_id = $1 ORDER BY created_at, id")
        .bind(account)
        .fetch_all(conn)
        .await
        .expect("read sessions")
}

fn errors_of(lines: &[Line], event: &str) -> usize {
    let needle = format!("event=\"{event}\"");
    lines
        .iter()
        .filter(|line| line.level == Level::ERROR && line.text.contains(&needle))
        .count()
}

#[tokio::test]
async fn one_event_of_each_kind_is_stored_with_ids_only_and_the_reuse_is_logged_once() {
    // Arrange
    let mut world = world().await;
    let account = world.known_identity(OIDC, SUBJECT).await;
    let session_service = sessions(&world.harness);
    let devices = grants(&mut world.harness, geo(&[]));
    let approver = member(&mut world.conn).await;
    let extra = fixtures::live_session(
        &mut world.conn,
        account,
        "extra-session",
        "browser",
        FAR_FUTURE,
        START.as_unix_micros(),
    )
    .await;
    let logs = LogCapture::install();

    // Act
    let (callback, binding) = authorized(&world, OIDC, SUBJECT, None, |grant| {
        grant.email = Some((EMAIL.to_owned(), true));
    })
    .await;
    let callback_values: Vec<String> = callback.0.iter().map(|(_, value)| value.clone()).collect();
    let outcome = world
        .service
        .complete(OIDC, callback, &binding, HOST, CLIENT_IP)
        .await
        .expect("sign-in");
    let SignInOutcome::Session { tokens, .. } = outcome else {
        panic!("a web sign-in gets a session");
    };
    let authorization = devices
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    let view = devices
        .lookup_for_approval(&approver, &authorization.user_code, ip("10.0.0.6"))
        .await
        .expect("lookup");
    devices
        .decide(&approver, view.grant_id, Decision::Approve)
        .await
        .expect("approve");
    session_service
        .revoke(account, extra, RevokeReason::User)
        .await
        .expect("revoke");
    let rotated = session_service
        .refresh(&tokens.refresh)
        .await
        .expect("rotation");
    world
        .harness
        .clock
        .tick(world.harness.config.refresh_reuse_grace * 2);
    let reuse = session_service.refresh(&tokens.refresh).await;

    // Assert
    assert!(
        matches!(reuse, Err(SessionError::ReuseDetected)),
        "{reuse:?}"
    );
    let rows = audit::rows(&mut world.conn).await;
    let kinds: Vec<&str> = rows.iter().map(|row| row.event.as_str()).collect();
    assert_eq!(
        kinds,
        vec![
            "sign_in",
            "device_approved",
            "session_revoked",
            "refresh_reuse"
        ],
        "{rows:?}"
    );
    let signed_in = session_ids(&mut world.conn, account).await;
    assert_eq!(rows[0].actor_account_id, Some(account));
    assert!(signed_in.contains(&rows[0].actor_session_id.expect("session id")));
    assert_eq!(rows[1].subject_id, Some(view.grant_id));
    assert_eq!(rows[2].subject_id, Some(extra));
    assert_eq!(
        rows[2].detail.as_deref(),
        Some(r#"{"scope": "one", "reason": "user"}"#)
    );
    assert_eq!(rows[3].actor_account_id, Some(account));
    let warns = logs.at(Level::WARN, "refresh token reuse");
    assert_eq!(warns.len(), 1, "{:?}", logs.lines());
    let secrets = [
        tokens.access.expose().to_owned(),
        tokens.refresh.expose().to_owned(),
        rotated.access.expose().to_owned(),
        rotated.refresh.expose().to_owned(),
        authorization.device_code.expose().to_owned(),
        authorization.user_code.as_str().to_owned(),
        binding.0.clone(),
        EMAIL.to_owned(),
    ]
    .into_iter()
    .chain(callback_values);
    let texts: Vec<String> = rows
        .iter()
        .map(|row| format!("{row:?}"))
        .chain(logs.lines().into_iter().map(|line| line.text))
        .collect();
    for secret in secrets {
        assert!(!secret.is_empty());
        for text in &texts {
            assert!(!text.contains(secret.as_str()), "{text}");
        }
    }
    drop((session_service, devices));
    world.finish().await;
}

#[tokio::test]
async fn with_a_failing_audit_writer_approval_and_sign_in_stand_and_log_while_a_revocation_fails_closed()
 {
    // Arrange
    let mut world = world().await;
    let account = world.known_identity(OIDC, SUBJECT).await;
    let session_service = sessions(&world.harness);
    let devices = grants(&mut world.harness, geo(&[]));
    let approver = member(&mut world.conn).await;
    let target = fixtures::live_session(
        &mut world.conn,
        account,
        "target-session",
        "browser",
        FAR_FUTURE,
        START.as_unix_micros(),
    )
    .await;
    let authorization = devices
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    audit::block_inserts(&mut world.conn).await;
    let logs = LogCapture::install();

    // Act
    let view = devices
        .lookup_for_approval(&approver, &authorization.user_code, ip("10.0.0.6"))
        .await
        .expect("lookup");
    let approved = devices
        .decide(&approver, view.grant_id, Decision::Approve)
        .await;
    let signed_in = sign_in(&world, OIDC, SUBJECT, None, |_| {}).await;
    let revoked = session_service
        .revoke(account, target, RevokeReason::User)
        .await;

    // Assert
    assert!(approved.is_ok(), "{approved:?}");
    assert!(
        matches!(signed_in, Ok(SignInOutcome::Session { .. })),
        "{signed_in:?}"
    );
    assert!(
        matches!(revoked, Err(SessionError::Unexpected(_))),
        "{revoked:?}"
    );
    assert_eq!(revocation(&mut world.conn, target).await, (false, None));
    let lines = logs.lines();
    assert_eq!(errors_of(&lines, "device_approved"), 1, "{lines:?}");
    assert_eq!(errors_of(&lines, "sign_in"), 1, "{lines:?}");
    assert!(audit::rows(&mut world.conn).await.is_empty());
    drop((session_service, devices));
    world.finish().await;
}
