//! AZ-3823 AC-1 (ST-07-1, AC-59, NFT-SEC-13) and AC-4 (AT-07-2 in its port form, AC-23, FT-N-46): the admin
//! operations at the 07 ports over the real 01 (account admin and authenticator) and 07a on a real PostgreSQL. The
//! callers' principals come from 01's authenticator over their stored session and device token. The only double
//! is the named degraded health contributor of AC-4. The HTTP statuses and the CLI exit codes of AT-07-2 run in
//! FT-N-46 and AZ-3665 (flagged concern of AZ-3823).

mod support;

use contracts::{
    AccountDeletionService, AccountState, Actor, AdminError, AdminService, Authenticator,
    DeletionError, HealthStatus, Page, Principal, PrincipalKind, Role,
};
use sqlx::PgConnection;
use sunnycloud_admin::admin_error_code;
use support::account_data::deletion_requested_at;
use support::harness::START;
use support::health_doubles::{TestContributor, as_ports};
use support::seed::{
    account, admin, audit_rows, device_token, identity, limit_of, quota, revoked_credentials,
    role_and_state, session,
};
use support::world::{World, device_credential, session_credential};
use uuid::Uuid;

/// Admin A with a session and a device token, and ordinary user U with a session.
struct Scene {
    world: World,
    a: Uuid,
    u: Uuid,
    a_session: Principal,
    a_device: Principal,
    u_session: Principal,
}

async fn authenticated(world: &World, credential: contracts::Credential) -> Principal {
    world
        .authenticator()
        .authenticate(credential)
        .await
        .expect("the stored credential authenticates")
}

async fn scene(world: World) -> Scene {
    let mut conn = world.fixture().await;
    let (a, u) = (Uuid::new_v4(), Uuid::new_v4());
    admin(&mut conn, a).await;
    let a_session_id = session(&mut conn, a).await;
    let a_device_id = device_token(&mut conn, a).await;
    account(
        &mut conn,
        u,
        "Ulla User",
        "user",
        "active",
        START.as_unix_micros(),
    )
    .await;
    quota(&mut conn, u, None, 0, 0).await;
    identity(&mut conn, u, "ulla@example.test", None).await;
    let u_session_id = session(&mut conn, u).await;
    drop(conn);
    let a_session = authenticated(&world, session_credential(a_session_id)).await;
    let a_device = authenticated(&world, device_credential(a_device_id)).await;
    let u_session = authenticated(&world, session_credential(u_session_id)).await;
    Scene {
        world,
        a,
        u,
        a_session,
        a_device,
        u_session,
    }
}

/// Role, state, limit and revoked credentials of `account`: what no refused call may change.
async fn standing(
    conn: &mut PgConnection,
    account: Uuid,
) -> ((String, String), Option<i64>, (i64, i64)) {
    (
        role_and_state(conn, account).await,
        limit_of(conn, account).await,
        revoked_credentials(conn, account).await,
    )
}

fn forbidden<T: std::fmt::Debug>(result: &Result<T, AdminError>) -> bool {
    matches!(result, Err(AdminError::Forbidden))
}

/// The error's variant, wire code and text, which a caller could tell apart.
fn not_found_shape(
    result: Result<(), AdminError>,
) -> Option<(Option<sc_error::ErrorCode>, String)> {
    match result {
        Err(error @ AdminError::NotFound) => Some((admin_error_code(&error), error.to_string())),
        _ => None,
    }
}

#[tokio::test]
async fn st_07_1_a_user_session_and_an_admin_device_token_are_forbidden_and_unknown_ids_are_one_not_found()
 {
    // Arrange
    let s = scene(World::new().await).await;
    let mut conn = s.world.fixture().await;
    let before = (
        standing(&mut conn, s.a).await,
        standing(&mut conn, s.u).await,
    );
    let callers = [
        Actor::Admin(s.u_session.clone()),
        Actor::Admin(s.a_device.clone()),
    ];
    let page = Page {
        offset: 0,
        limit: 20,
    };

    // Act
    let mut refusals = Vec::new();
    for caller in &callers {
        let admin = &s.world.admin;
        refusals.push(forbidden(&admin.list_users(caller, page).await));
        for target in [s.u, s.a] {
            let role = admin
                .update_user(caller, target, Some(Role::Admin), None)
                .await;
            let demote = admin
                .update_user(caller, target, Some(Role::User), None)
                .await;
            let state = admin
                .update_user(caller, target, None, Some(AccountState::Disabled))
                .await;
            let quota = admin.set_quota(caller, target, Some(1)).await;
            refusals.extend([&role, &demote, &state, &quota].map(forbidden));
        }
        refusals.push(forbidden(&admin.health(caller).await));
    }
    let a = Actor::Admin(s.a_session.clone());
    let mut shapes = Vec::new();
    for unknown in [Uuid::nil(), Uuid::new_v4()] {
        let admin = &s.world.admin;
        shapes.push([
            not_found_shape(
                admin
                    .update_user(&a, unknown, Some(Role::Admin), None)
                    .await,
            ),
            not_found_shape(
                admin
                    .update_user(&a, unknown, None, Some(AccountState::Disabled))
                    .await,
            ),
            not_found_shape(admin.set_quota(&a, unknown, Some(1 << 30)).await),
        ]);
    }

    // Assert
    assert_eq!(
        (s.u_session.role, s.a_device.kind, s.a_device.role),
        (Role::User, PrincipalKind::Device, Role::Admin),
        "the principals are the ones 01 issues"
    );
    assert_eq!(refusals, vec![true; 20]);
    assert!(shapes[0].iter().all(Option::is_some), "{shapes:?}");
    assert_eq!(shapes[0], shapes[1], "missing and random ids differ");
    let after = (
        standing(&mut conn, s.a).await,
        standing(&mut conn, s.u).await,
    );
    assert_eq!(after, before);
    // The optional refusal entry of ST-07-1 is not built: a refused call writes no audit row at all.
    assert_eq!(audit_rows(&mut conn).await.len(), 0);
    drop(conn);
    s.world.finish().await;
}

#[tokio::test]
async fn at_07_2_the_only_active_admin_survives_every_admin_account_and_operator_path() {
    // Arrange
    let blob_volume = TestContributor::answering(
        "blob volume",
        HealthStatus::Degraded,
        "below the free floor",
    );
    let database = TestContributor::answering("database", HealthStatus::Ok, "reachable");
    let world = World::with_health(as_ports(&[database, blob_volume])).await;
    let s = scene(world).await;
    let mut conn = s.world.fixture().await;
    let before = standing(&mut conn, s.a).await;
    let own = Actor::Admin(s.a_session.clone());

    // Act
    let admin = &s.world.admin;
    let updates = [
        admin.update_user(&own, s.a, Some(Role::User), None).await,
        admin
            .update_user(&own, s.a, None, Some(AccountState::Disabled))
            .await,
        admin
            .update_user(&Actor::Operator, s.a, Some(Role::User), None)
            .await,
    ];
    let deletions = [
        s.world
            .deletion
            .request(&s.a_session, &s.a.to_string())
            .await,
        s.world.deletion.request_as_operator(s.a).await,
    ];
    let health = admin.health(&own).await.expect("health report");

    // Assert
    for update in &updates {
        assert!(matches!(update, Err(AdminError::LastAdmin)), "{update:?}");
    }
    for deletion in &deletions {
        assert!(
            matches!(deletion, Err(DeletionError::LastAdmin)),
            "{deletion:?}"
        );
    }
    assert_eq!(standing(&mut conn, s.a).await, before);
    assert_eq!(deletion_requested_at(&mut conn, s.a).await, None);
    assert_eq!(audit_rows(&mut conn).await.len(), 0);
    let statuses: Vec<(&str, HealthStatus)> = health
        .items
        .iter()
        .map(|(name, item)| (name.as_str(), item.status))
        .collect();
    assert_eq!(
        statuses,
        [
            ("database", HealthStatus::Ok),
            ("blob volume", HealthStatus::Degraded)
        ]
    );
    drop(conn);
    s.world.finish().await;
}
