//! AC-1 and AC-2 (IT-01-1): one answer for every dead credential, from one indexed read and no write; live
//! credentials give a principal. Real PostgreSQL, the manual clock, the component on role `sunnycloud`.

mod support;

use std::time::Duration;

use contracts::{Authenticator, AuthenticatorError, Credential, Principal, PrincipalKind, Role};
use sqlx::{Connection, PgConnection};
use support::auth::{FAR_FUTURE, authenticator, authenticator_with_ttl, device, session};
use support::fixtures;
use support::statements::StatementLog;
use support::{Harness, START};
use uuid::Uuid;

struct World {
    harness: Harness,
    account: Uuid,
    session_id: Uuid,
    device_id: Uuid,
    conn: PgConnection,
}

/// Account A (active member) with live session `s` and live device token `d`.
async fn world() -> World {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let at = START.as_unix_micros();
    let account = Uuid::new_v4();
    fixtures::account(&mut conn, account, at).await;
    let session_id =
        fixtures::live_session(&mut conn, account, "s", "Firefox on Linux", FAR_FUTURE, at).await;
    let device_id = fixtures::named_device(&mut conn, account, "d", "laptop", at).await;
    World {
        harness,
        account,
        session_id,
        device_id,
        conn,
    }
}

impl World {
    async fn finish(self) {
        self.conn.close().await.expect("close fixture connection");
        assert_eq!(self.harness.finish().await, 0, "leaked connections");
    }
}

/// The 8 dead credentials of IT-01-1, each paired with the call that presents it.
async fn dead_credentials(world: &mut World) -> Vec<(&'static str, Credential)> {
    let at = START.as_unix_micros();
    let conn = &mut world.conn;
    let expiring = START.as_unix_micros() + 900_000_000;
    fixtures::live_session(conn, world.account, "expired", "old", expiring, at).await;
    let revoked_session =
        fixtures::live_session(conn, world.account, "revoked-s", "old", FAR_FUTURE, at).await;
    fixtures::revoke_session(conn, revoked_session, at).await;
    let revoked_device = fixtures::named_device(conn, world.account, "revoked-d", "old", at).await;
    fixtures::revoke_device(conn, revoked_device, at).await;
    for (state, token) in [("disabled", "of-disabled"), ("deleting", "of-deleting")] {
        let account = Uuid::new_v4();
        fixtures::account_in(conn, account, "user", state, at).await;
        fixtures::live_session(conn, account, token, "other", FAR_FUTURE, at).await;
    }
    world.harness.clock.tick(Duration::from_secs(901));
    vec![
        ("never issued", session("never-issued")),
        ("expired session", session("expired")),
        ("revoked session", session("revoked-s")),
        ("revoked device", device("revoked-d")),
        ("session of a disabled account", session("of-disabled")),
        ("session of a deleting account", session("of-deleting")),
        ("malformed", session("wk_%%not a token%%")),
        ("empty", session("")),
    ]
}

#[tokio::test]
async fn every_dead_credential_gets_the_same_unauthenticated_after_one_read() {
    // Arrange
    let mut world = world().await;
    let credentials = dead_credentials(&mut world).await;
    let authenticator = authenticator_with_ttl(&mut world.harness, Duration::ZERO);
    let log = StatementLog::install();

    for (name, credential) in credentials {
        // Act
        let result = authenticator.authenticate(credential).await;
        let statements = log.take_data_statements();

        // Assert
        assert!(
            matches!(result, Err(AuthenticatorError::Unauthenticated)),
            "{name}: {result:?}"
        );
        assert_eq!(format!("{result:?}"), "Err(Unauthenticated)", "{name}");
        assert_eq!(statements.len(), 1, "{name}: {statements:?}");
        assert!(
            statements[0].starts_with("SELECT") && statements[0].contains("token_hash = $1"),
            "{name}: {}",
            statements[0]
        );
        for write in ["INSERT", "UPDATE", "DELETE"] {
            assert!(!statements[0].contains(write), "{name}: {}", statements[0]);
        }
    }
    drop(authenticator);
    world.finish().await;
}

#[tokio::test]
async fn the_probe_of_each_credential_kind_uses_its_unique_hash_index() {
    // Arrange
    let mut world = world().await;
    let authenticator = authenticator_with_ttl(&mut world.harness, Duration::ZERO);
    let log = StatementLog::install();
    let mut plans = Vec::new();

    for (credential, index) in [
        (session("s"), "sessions_token_hash_key"),
        (device("d"), "device_tokens_token_hash_key"),
    ] {
        // Act
        authenticator
            .authenticate(credential)
            .await
            .expect("live credential");
        let statement = log.take_data_statements().remove(0);
        let mut conn = world.harness.fixture_conn().await;
        sqlx::raw_sql("SET enable_seqscan = off")
            .execute(&mut conn)
            .await
            .expect("disable sequential scans for the plan");
        let plan: Vec<String> =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("EXPLAIN {statement}")))
                .bind(vec![0u8; 32])
                .fetch_all(&mut conn)
                .await
                .expect("explain the probe");
        conn.close().await.expect("close plan connection");
        log.take();
        plans.push((index, plan.join("\n")));
    }

    // Assert
    for (index, plan) in plans {
        assert!(
            plan.contains(&format!("Index Scan using {index}")),
            "{plan}"
        );
    }
    drop(authenticator);
    world.finish().await;
}

#[tokio::test]
async fn live_session_and_device_give_a_principal() {
    // Arrange
    let world = world().await;
    let authenticator = authenticator(&world.harness);

    // Act
    let session = authenticator.authenticate(session("s")).await;
    let device = authenticator.authenticate(device("d")).await;

    // Assert
    assert_eq!(
        session.expect("live session"),
        Principal {
            account_id: world.account,
            kind: PrincipalKind::Session,
            session_or_device_id: world.session_id,
            role: Role::User,
            label: "Firefox on Linux".to_owned(),
        }
    );
    assert_eq!(
        device.expect("live device token"),
        Principal {
            account_id: world.account,
            kind: PrincipalKind::Device,
            session_or_device_id: world.device_id,
            role: Role::User,
            label: "laptop".to_owned(),
        }
    );
    drop(authenticator);
    world.finish().await;
}

#[tokio::test]
async fn second_authenticate_inside_the_ttl_reads_nothing() {
    // Arrange
    let world = world().await;
    let authenticator = authenticator(&world.harness);
    let log = StatementLog::install();
    let first = authenticator.authenticate(session("s")).await;
    log.take();

    // Act
    world.harness.clock.tick(Duration::from_millis(100));
    let second = authenticator.authenticate(session("s")).await;

    // Assert
    assert_eq!(first.expect("first"), second.expect("second"));
    assert_eq!(log.take_data_statements(), Vec::<String>::new());
    drop(authenticator);
    world.finish().await;
}

#[tokio::test]
async fn authenticate_after_the_ttl_reads_again() {
    // Arrange
    let world = world().await;
    let authenticator = authenticator(&world.harness);
    let log = StatementLog::install();
    authenticator
        .authenticate(session("s"))
        .await
        .expect("first");
    log.take();

    // Act
    world
        .harness
        .clock
        .tick(world.harness.config.session_cache_ttl);
    authenticator
        .authenticate(session("s"))
        .await
        .expect("second");

    // Assert
    assert_eq!(log.take_data_statements().len(), 1);
    drop(authenticator);
    world.finish().await;
}

#[test]
fn credential_has_exactly_two_variants() {
    // Arrange
    fn variant(credential: &Credential) -> &'static str {
        match credential {
            Credential::Session(_) => "session",
            Credential::Device(_) => "device",
        }
    }

    // Act
    let names = [variant(&session("x")), variant(&device("x"))];

    // Assert
    assert_eq!(names, ["session", "device"]);
}
