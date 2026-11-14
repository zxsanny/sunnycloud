//! AZ-3819 AC-2, AC-3, AC-4 (IT-07-8, ST-07-2) and the operator path: the deletion request refuses a device
//! token and a wrong confirmation without changing anything, and otherwise marks the account `deleting`, ends
//! its access and leaves its data for the purger. Real 01, 02 and 07a on a real PostgreSQL.

mod support;

use std::time::{Duration, Instant};

use contracts::{
    AccountDeletionService, Authenticator, AuthenticatorError, CommitActor, DeletionAccepted,
    DeletionError, Principal, PrincipalKind, Role, TreeCommit, TreeCommitError,
};
use sc_clock::Clock;
use sqlx::PgConnection;
use support::account_data::{
    FILE_NAME, data_rows, deletion_requested_at, linked_identity, live_file, root, share_link,
};
use support::harness::START;
use support::seed::{
    account, audit_rows, device_token, quota, revoked_credentials, role_and_state, session,
};
use support::world::{World, device_credential, session_credential};
use uuid::Uuid;

const DISPLAY_NAME: &str = "Ursula Fixture";
const EMAIL: &str = "ursula@example.test";
const DEADLINE: Duration = Duration::from_secs(30);
const RESPONSE_BOUND: Duration = Duration::from_secs(1);

/// A user with an identity, a root folder holding a shared file, 2 sessions and 1 device token.
struct User {
    id: Uuid,
    sessions: [Uuid; 2],
    device: Uuid,
}

async fn user(conn: &mut PgConnection) -> User {
    let id = Uuid::new_v4();
    account(
        conn,
        id,
        DISPLAY_NAME,
        "user",
        "active",
        START.as_unix_micros(),
    )
    .await;
    quota(conn, id, None, 1, 0).await;
    linked_identity(conn, id, "google", Some(EMAIL), START.as_unix_micros()).await;
    let folder = root(conn, id).await;
    let file = live_file(conn, id, folder).await;
    share_link(conn, id, file).await;
    let sessions = [session(conn, id).await, session(conn, id).await];
    let device = device_token(conn, id).await;
    User {
        id,
        sessions,
        device,
    }
}

impl User {
    fn session_principal(&self) -> Principal {
        Principal {
            account_id: self.id,
            kind: PrincipalKind::Session,
            session_or_device_id: self.sessions[0],
            role: Role::User,
            label: String::new(),
        }
    }

    fn device_principal(&self) -> Principal {
        Principal {
            kind: PrincipalKind::Device,
            session_or_device_id: self.device,
            ..self.session_principal()
        }
    }

    /// Whether each of the 3 credentials authenticates, read by a fresh authenticator.
    async fn credentials_live(&self, world: &World) -> [bool; 3] {
        let authenticator = world.authenticator();
        let mut live = [false; 3];
        let credentials = [
            session_credential(self.sessions[0]),
            session_credential(self.sessions[1]),
            device_credential(self.device),
        ];
        for (slot, credential) in live.iter_mut().zip(credentials) {
            *slot = match authenticator.authenticate(credential).await {
                Ok(principal) => principal.account_id == self.id,
                Err(AuthenticatorError::Unauthenticated) => false,
                Err(error) => panic!("unexpected authenticator failure: {error:?}"),
            };
        }
        live
    }
}

/// State, deletion time, revoked credentials and audit row count: everything a refused request must not change.
async fn untouched(
    conn: &mut PgConnection,
    account: Uuid,
) -> (String, Option<i64>, (i64, i64), usize) {
    (
        role_and_state(conn, account).await.1,
        deletion_requested_at(conn, account).await,
        revoked_credentials(conn, account).await,
        audit_rows(conn).await.len(),
    )
}

fn expected_deadline() -> DeletionAccepted {
    DeletionAccepted {
        deadline: START.checked_add(DEADLINE).expect("deadline fits"),
    }
}

#[tokio::test]
async fn a_device_token_with_the_right_confirmation_is_forbidden_and_changes_nothing() {
    // Arrange
    let world = World::new().await;
    let mut conn = world.fixture().await;
    let u = user(&mut conn).await;
    let confirmation = u.id.to_string();

    // Act
    let result = world
        .deletion
        .request(&u.device_principal(), &confirmation)
        .await;

    // Assert
    assert!(
        matches!(result, Err(DeletionError::Forbidden)),
        "{result:?}"
    );
    assert_eq!(
        untouched(&mut conn, u.id).await,
        ("active".to_owned(), None, (0, 0), 0)
    );
    assert_eq!(u.credentials_live(&world).await, [true; 3]);
    drop(conn);
    world.finish().await;
}

#[tokio::test]
async fn every_confirmation_other_than_the_exact_own_id_is_a_mismatch_and_changes_nothing() {
    // Arrange
    let world = World::new().await;
    let mut conn = world.fixture().await;
    let u = user(&mut conn).await;
    let other = user(&mut conn).await;
    let wrong = [
        String::new(),
        "   ".to_owned(),
        u.id.to_string().to_uppercase(),
        other.id.to_string(),
    ];

    for confirmation in &wrong {
        // Act
        let result = world
            .deletion
            .request(&u.session_principal(), confirmation)
            .await;

        // Assert
        assert!(
            matches!(result, Err(DeletionError::ConfirmationMismatch)),
            "{confirmation:?}: {result:?}"
        );
    }
    assert_eq!(
        untouched(&mut conn, u.id).await,
        ("active".to_owned(), None, (0, 0), 0)
    );
    assert_eq!(untouched(&mut conn, other.id).await.0, "active");
    assert_eq!(u.credentials_live(&world).await, [true; 3]);
    drop(conn);
    world.finish().await;
}

#[tokio::test]
async fn the_right_confirmation_marks_deleting_ends_access_and_keeps_the_data() {
    // Arrange
    let world = World::new().await;
    let mut conn = world.fixture().await;
    let u = user(&mut conn).await;
    let bystander = user(&mut conn).await;
    let data_before = data_rows(&mut conn, u.id).await;
    let confirmation = u.id.to_string();
    let started = Instant::now();

    // Act
    let accepted = world
        .deletion
        .request(&u.session_principal(), &confirmation)
        .await;
    let elapsed = started.elapsed();

    // Assert
    assert_eq!(accepted.expect("accepted"), expected_deadline());
    assert!(elapsed < RESPONSE_BOUND, "took {elapsed:?}");
    assert_eq!(role_and_state(&mut conn, u.id).await.1, "deleting");
    assert_eq!(
        deletion_requested_at(&mut conn, u.id).await,
        Some(START.as_unix_micros())
    );
    assert_eq!(u.credentials_live(&world).await, [false; 3]);
    assert_eq!(data_before, [2, 1, 1, 1]);
    assert_eq!(data_rows(&mut conn, u.id).await, data_before);
    let commit = world
        .commit
        .publish(
            CommitActor {
                account_id: u.id,
                device_label: "laptop".to_owned(),
            },
            Vec::new(),
            Vec::new(),
            world.clock.now_utc(),
        )
        .await;
    assert!(
        matches!(commit, Err(TreeCommitError::AccountNotActive)),
        "{commit:?}"
    );
    let audit = audit_rows(&mut conn).await;
    assert_eq!(audit.len(), 1);
    let row = &audit[0];
    assert_eq!(
        (
            row.event.as_str(),
            row.actor_account_id,
            row.actor_session_id,
            row.subject_type.as_deref(),
            row.subject_id,
            row.action.as_deref(),
        ),
        (
            "admin_action",
            Some(u.id),
            Some(u.sessions[0]),
            Some("account"),
            Some(u.id),
            Some("deletion_requested"),
        )
    );
    for personal in [DISPLAY_NAME, EMAIL, FILE_NAME] {
        assert!(!row.whole_row.contains(personal), "{}", row.whole_row);
    }
    assert_eq!(untouched(&mut conn, bystander.id).await.0, "active");
    assert_eq!(bystander.credentials_live(&world).await, [true; 3]);
    drop(conn);
    world.finish().await;
}

#[tokio::test]
async fn the_operator_request_needs_no_confirmation_and_reaches_the_same_end_state() {
    // Arrange
    let world = World::new().await;
    let mut conn = world.fixture().await;
    let v = user(&mut conn).await;
    let data_before = data_rows(&mut conn, v.id).await;

    // Act
    let accepted = world.deletion.request_as_operator(v.id).await;

    // Assert
    assert_eq!(accepted.expect("accepted"), expected_deadline());
    assert_eq!(role_and_state(&mut conn, v.id).await.1, "deleting");
    assert_eq!(v.credentials_live(&world).await, [false; 3]);
    assert_eq!(data_rows(&mut conn, v.id).await, data_before);
    let audit = audit_rows(&mut conn).await;
    assert_eq!(audit.len(), 1);
    assert_eq!(
        (
            audit[0].actor_account_id,
            audit[0].actor_session_id,
            audit[0].subject_id,
            audit[0].action.as_deref(),
        ),
        (None, None, Some(v.id), Some("deletion_requested"))
    );
    drop(conn);
    world.finish().await;
}
