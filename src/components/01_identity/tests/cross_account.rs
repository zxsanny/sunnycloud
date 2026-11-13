//! ST-01-1 (AC-59, NFT-SEC-13, NFT-SEC-14), AZ-3692 AC-1: account B, holding the ids of account A's sessions,
//! devices and device grants, reaches none of them through `SessionService` or `DeviceGrantService`. Both
//! accounts, their 2 sessions and 2 device tokens each, and A's grants are made through the real services (setup
//! token, member invite, sign-in, device grant) with the real lifecycle hooks on PostgreSQL 18.

mod support;

use std::collections::BTreeSet;

use contracts::{
    Authenticator, Decision, DeviceGrantError, DeviceGrantService, Principal, RevokeReason,
    SessionService, SessionTokens,
};
use sunnycloud_identity::{IdentityAuthenticator, IdentityDeviceGrants, IdentitySessions};
use support::auth::{authenticator, device, session};
use support::device::{approve, geo, grant_status, grants, ip};
use support::invites::invites;
use support::onboarding::{all_rows, another_session, first_admin, member_by_invite, principal_of};
use support::registration::Rig;
use support::{audit, sessions::sessions};
use uuid::Uuid;

const POOL: u32 = 8;

/// One account: its first session's principal, both sessions' tokens and ids, its device tokens with their ids
/// and the grants they came from.
struct Member {
    principal: Principal,
    sessions: Vec<(Uuid, SessionTokens)>,
    devices: Vec<(Uuid, String)>,
    grants: Vec<Uuid>,
}

struct World {
    rig: Rig,
    sessions: IdentitySessions,
    grants: IdentityDeviceGrants,
    a: Member,
    b: Member,
}

/// A second session of `subject`, then two devices approved by the first session.
async fn member(
    rig: &Rig,
    service: &IdentityDeviceGrants,
    authenticator: &IdentityAuthenticator,
    first: SessionTokens,
    subject: &str,
) -> Member {
    let second = another_session(rig, subject).await;
    let principal = principal_of(authenticator, &first).await;
    let second_id = principal_of(authenticator, &second)
        .await
        .session_or_device_id;
    let mut member = Member {
        sessions: vec![(principal.session_or_device_id, first), (second_id, second)],
        principal,
        devices: Vec::new(),
        grants: Vec::new(),
    };
    for _ in 0..2 {
        let authorization = service
            .request("lab-1", "linux", ip("10.0.0.5"))
            .await
            .expect("request");
        let grant = approve(
            service,
            &member.principal,
            &authorization.user_code,
            ip("198.51.100.9"),
        )
        .await;
        let token = service
            .poll(&authorization.device_code)
            .await
            .expect("poll of an approved grant");
        let id = authenticator
            .authenticate(device(token.0.expose()))
            .await
            .expect("the device token authenticates")
            .session_or_device_id;
        member.devices.push((id, token.0.expose().to_owned()));
        member.grants.push(grant);
    }
    member
}

/// A is the first admin (setup token), B a member by A's invite; 2 sessions and 2 devices each.
async fn two_accounts() -> World {
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let service = grants(&mut rig.world.harness, geo(&[]));
    let authenticator = authenticator(&rig.world.harness);
    let operator = invites(&rig.world.harness);
    let a_first = first_admin(&rig, &operator, "identity-a").await;
    let a = member(&rig, &service, &authenticator, a_first, "identity-a").await;
    let b_first = member_by_invite(&rig, &operator, a.principal.account_id, "identity-b").await;
    let b = member(&rig, &service, &authenticator, b_first, "identity-b").await;
    World {
        sessions: sessions(&rig.world.harness),
        rig,
        grants: service,
        a,
        b,
    }
}

impl World {
    async fn finish(self) {
        let World {
            rig,
            sessions,
            grants,
            ..
        } = self;
        drop((sessions, grants));
        rig.finish().await;
    }
}

fn ids<T>(items: &[(Uuid, T)]) -> BTreeSet<Uuid> {
    items.iter().map(|(id, _)| *id).collect()
}

#[tokio::test]
async fn account_b_with_a_ids_changes_lists_and_decides_nothing_of_a() {
    // Arrange
    let mut world = two_accounts().await;
    let (a, b) = (&world.a, &world.b);
    let rows_before = all_rows(&mut world.rig.world.conn, true).await;
    let audit_before = audit::rows(&mut world.rig.world.conn).await;

    // Act
    let mut revokes = Vec::new();
    for (id, _) in &a.sessions {
        let result = world
            .sessions
            .revoke(b.principal.account_id, *id, RevokeReason::User)
            .await;
        revokes.push(result);
    }
    let listed = world
        .sessions
        .list(b.principal.account_id)
        .await
        .expect("list");
    let mut refusals = Vec::new();
    for (id, _) in &a.devices {
        let result = world
            .grants
            .revoke_device(b.principal.account_id, *id)
            .await;
        refusals.push(format!("{result:?}"));
    }
    let devices = world
        .grants
        .list_devices(b.principal.account_id)
        .await
        .expect("list devices");
    for grant in &a.grants {
        for decision in [Decision::Approve, Decision::Deny] {
            let result = world.grants.decide(&b.principal, *grant, decision).await;
            refusals.push(format!("{result:?}"));
        }
    }

    // Assert
    assert!(revokes.iter().all(Result::is_ok), "{revokes:?}");
    assert_eq!(
        refusals,
        [
            vec![format!("{:?}", Err::<(), _>(DeviceGrantError::NotFound)); 2],
            vec![format!("{:?}", Err::<(), _>(DeviceGrantError::AlreadyDecided)); 4],
        ]
        .concat()
    );
    let listed: BTreeSet<Uuid> = listed.iter().map(|info| info.id).collect();
    let devices: BTreeSet<Uuid> = devices.iter().map(|info| info.id).collect();
    assert_eq!(listed, ids(&b.sessions));
    assert_eq!(devices, ids(&b.devices));
    assert_eq!(all_rows(&mut world.rig.world.conn, true).await, rows_before);
    assert_eq!(audit::rows(&mut world.rig.world.conn).await, audit_before);
    let fresh = authenticator(&world.rig.world.harness);
    for (_, tokens) in &a.sessions {
        let principal = fresh.authenticate(session(tokens.access.expose())).await;
        assert_eq!(
            principal.expect("A's session").account_id,
            a.principal.account_id
        );
    }
    for (_, token) in &a.devices {
        let principal = fresh.authenticate(device(token)).await;
        assert_eq!(
            principal.expect("A's device").account_id,
            a.principal.account_id
        );
    }
    drop(fresh);
    world.finish().await;
}

#[tokio::test]
#[ignore = "DEFECT: AZ-3692 AC-1 decide on a pending grant: any session that holds the grant id can approve or deny \
            it (B's decide on the grant A looked up returns Ok and binds A's device to B); lookup_for_approval does \
            not bind the grant to the looking account, which needs a device_grants column (schema change)"]
async fn account_b_cannot_decide_a_pending_grant_that_a_looked_up() {
    // Arrange
    let mut world = two_accounts().await;
    let authorization = world
        .grants
        .request("lab-2", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    let view = world
        .grants
        .lookup_for_approval(
            &world.a.principal,
            &authorization.user_code,
            ip("198.51.100.9"),
        )
        .await
        .expect("A looks the grant up");
    let audit_before = audit::rows(&mut world.rig.world.conn).await;

    // Act
    let approved = world
        .grants
        .decide(&world.b.principal, view.grant_id, Decision::Approve)
        .await;
    let denied = world
        .grants
        .decide(&world.b.principal, view.grant_id, Decision::Deny)
        .await;

    // Assert
    assert!(approved.is_err(), "{approved:?}");
    assert!(denied.is_err(), "{denied:?}");
    let status = grant_status(&mut world.rig.world.conn, &authorization.device_code).await;
    assert_eq!(status, "pending");
    assert_eq!(audit::rows(&mut world.rig.world.conn).await, audit_before);
    world.finish().await;
}
