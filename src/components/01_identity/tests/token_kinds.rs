//! AZ-4268 AC-2: a token of one kind, presented where another kind is expected, is refused as an unknown token of
//! the expected kind is. The tokens are minted through the real services on PostgreSQL 18; the hash covers the
//! type prefix, so a store of one kind never holds the hash of another kind's token, and the invite and setup
//! fields of a sign-in, whose tokens share one table, refuse each other's tokens from the text.

mod support;

use std::fmt::Debug;

use contracts::{
    Authenticator, ClientKind, DeviceGrantService, OperatorIdentityOps, SessionService, SignInError,
};
use sc_error::ErrorCode;
use sc_ids::{RawToken, TokenKind};
use support::auth::{authenticator, device, session};
use support::device::{geo, grants, ip, mint};
use support::invites::invites;
use support::onboarding::{first_admin, principal_of};
use support::registration::Rig;
use support::sessions::{refresh_token, sessions};
use support::sign_in::{CHALLENGE, OIDC, begin_request};

const POOL: u32 = 5;
const SUBJECT: &str = "identity-kinds";

fn answer<T: Debug, E: Debug>(result: Result<T, E>) -> String {
    format!("{result:?}")
}

fn presented(kind: TokenKind, value: &str) -> RawToken {
    RawToken::from_presented(kind, value.to_owned())
}

#[tokio::test]
async fn a_token_presented_as_another_kind_gets_the_answer_of_an_unknown_token_of_that_kind() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let operator = invites(&rig.world.harness);
    let service = grants(&mut rig.world.harness, geo(&[]));
    let auth = authenticator(&rig.world.harness);
    let refresher = sessions(&rig.world.harness);
    let tokens = first_admin(&rig, &operator, SUBJECT).await;
    let admin = principal_of(&auth, &tokens).await;
    let device_token = mint(&service, &admin).await.0;
    let pending = service
        .request("lab-2", "linux", ip("10.0.0.5"))
        .await
        .expect("request");
    let login_code = rig
        .world
        .mobile_code(ClientKind::Android, SUBJECT, CHALLENGE)
        .await;
    let (access, refresh) = (tokens.access.expose(), tokens.refresh.expose());

    // Act
    let as_session = [
        device_token.expose(),
        refresh,
        login_code.expose(),
        "sc_ses_unknown",
    ];
    let mut session_answers = Vec::new();
    for text in as_session {
        session_answers.push(answer(auth.authenticate(session(text)).await));
    }
    let mut device_answers = Vec::new();
    for text in [access, pending.device_code.expose(), "sc_dev_unknown"] {
        device_answers.push(answer(auth.authenticate(device(text)).await));
    }
    let mut refresh_answers = Vec::new();
    for text in [access, login_code.expose(), "sc_ref_unknown"] {
        refresh_answers.push(answer(refresher.refresh(&refresh_token(text)).await));
    }
    let mut poll_answers = Vec::new();
    for text in [device_token.expose(), access, "sc_dcd_unknown"] {
        let code = presented(TokenKind::DeviceCode, text);
        poll_answers.push(answer(service.poll(&code).await));
    }

    // Assert
    for answers in [
        &session_answers,
        &device_answers,
        &refresh_answers,
        &poll_answers,
    ] {
        let unknown = answers.last().expect("the unknown token's answer");
        assert!(unknown.starts_with("Err("), "{unknown}");
        assert!(answers.iter().all(|other| other == unknown), "{answers:?}");
    }
    let own = auth.authenticate(session(access)).await;
    assert_eq!(own.expect("the access token").account_id, admin.account_id);
    drop((operator, service, auth, refresher));
    rig.finish().await;
}

#[tokio::test]
async fn a_setup_token_in_the_invite_field_and_an_invite_in_the_setup_field_are_refused_at_begin() {
    // Arrange
    let mut rig = Rig::new(POOL, |_| {}, Vec::new()).await;
    let operator = invites(&rig.world.harness);
    let setup = operator
        .ensure_setup_token()
        .await
        .expect("ensure")
        .expect("a setup token while no account exists");
    let mut setup_as_invite = begin_request(OIDC, "/files");
    setup_as_invite.invite_token = Some(presented(TokenKind::Invite, setup.expose()));
    let mut invite_as_setup = begin_request(OIDC, "/files");
    invite_as_setup.setup_token = Some(presented(TokenKind::Setup, "sc_inv_any-invite"));
    let mut own_field = begin_request(OIDC, "/files");
    own_field.setup_token = Some(presented(TokenKind::Setup, setup.expose()));

    // Act
    let refused_setup = rig.world.service.begin(setup_as_invite).await;
    let refused_invite = rig.world.service.begin(invite_as_setup).await;
    let states_after_refusals = rig.world.counts().await.oauth_states;
    let accepted = rig.world.service.begin(own_field).await;

    // Assert
    for refused in [&refused_setup, &refused_invite] {
        assert!(
            matches!(
                refused,
                Err(SignInError::TrustedFailure {
                    code: ErrorCode::InvalidRequest
                })
            ),
            "{refused:?}"
        );
    }
    assert_eq!(states_after_refusals, 0);
    assert!(accepted.is_ok(), "{accepted:?}");
    drop(operator);
    rig.finish().await;
}
