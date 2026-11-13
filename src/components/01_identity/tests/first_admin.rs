//! AT-01-1 (AC-19, AC-23, AC-75; FT-P-64, FT-P-65), AZ-3692 AC-5: on an empty database an operator bootstraps the
//! first admin with the setup token, the admin onboards a member by invite, and an identity without an invite is
//! refused, using only the ports (`OperatorIdentityOps`, `InviteService`, `SignInService`, `Authenticator`,
//! `AccountAdmin`). The in-process provider stands in for mock-idp; the real lifecycle hooks run on PostgreSQL 18.

mod support;

use contracts::{
    AccountAdmin, AccountFilter, InviteService, OperatorIdentityOps, Page, PrincipalKind, Role,
    SignInError,
};
use support::accounts::accounts;
use support::auth::authenticator;
use support::invites::{invites, sign_in_with_setup};
use support::onboarding::{principal_of, session_tokens};
use support::registration::{Rig, sign_in};
use support::sign_in::OIDC;

const POOL: u32 = 5;
const ALL: Page = Page {
    offset: 0,
    limit: 100,
};

#[tokio::test]
async fn setup_token_makes_the_first_admin_an_invite_a_member_and_no_invite_nothing() {
    // Arrange
    let rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let operator = invites(&rig.world.harness);
    let admin_port = accounts(&rig.world.harness);
    let authenticator = authenticator(&rig.world.harness);
    let empty = admin_port
        .list_accounts(AccountFilter::default(), ALL)
        .await
        .expect("list accounts");

    // Act
    let setup = operator
        .ensure_setup_token()
        .await
        .expect("ensure the setup token")
        .expect("a setup token on an empty database");
    let first = sign_in_with_setup(&rig.world, OIDC, "identity-1", setup.expose()).await;
    let admin = principal_of(&authenticator, &session_tokens(first)).await;
    let invite = operator
        .create_invite(admin.account_id, "member")
        .await
        .expect("create a member invite");
    let second = sign_in(
        &rig.world,
        OIDC,
        "identity-2",
        Some(invite.0.expose()),
        |_| {},
    )
    .await;
    let member = principal_of(&authenticator, &session_tokens(second)).await;
    let third = sign_in(&rig.world, OIDC, "identity-3", None, |_| {}).await;
    let listed = admin_port
        .list_accounts(AccountFilter::default(), ALL)
        .await
        .expect("list accounts");

    // Assert
    assert!(empty.is_empty(), "{empty:?}");
    assert_eq!(
        (admin.kind, admin.role),
        (PrincipalKind::Session, Role::Admin)
    );
    assert_eq!(
        (member.kind, member.role),
        (PrincipalKind::Session, Role::User)
    );
    assert_ne!(admin.account_id, member.account_id);
    assert!(
        matches!(third, Err(SignInError::InvitationRequired)),
        "{third:?}"
    );
    let mut roles: Vec<(uuid::Uuid, Role)> =
        listed.iter().map(|info| (info.id, info.role)).collect();
    roles.sort_by_key(|(_, role)| *role == Role::User);
    assert_eq!(
        roles,
        vec![
            (admin.account_id, Role::Admin),
            (member.account_id, Role::User)
        ]
    );
    drop((authenticator, admin_port, operator));
    rig.finish().await;
}
