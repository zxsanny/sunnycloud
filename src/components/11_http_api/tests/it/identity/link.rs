//! The provider callback as the end of an identity link: a request with a live session cookie calls
//! `complete_link` and lands on the account page (`api_v1_surface.md` Open point 34).

use contracts::{Linked, SignInError, SignInOutcome};
use sc_error::ErrorCode;

use super::support::*;
use crate::auth::support::{FAULT_TOKEN, MEMBER_ACCOUNT, MEMBER_TOKEN};
use crate::harness::SESSION_COOKIE;

const COMPLETE: &str = "SignInService::complete";
const COMPLETE_LINK: &str = "SignInService::complete_link";
const CALLBACK: &str = "/api/v1/auth/providers/github/callback";

fn session_outcome() -> Result<SignInOutcome, SignInError> {
    Ok(SignInOutcome::Session {
        tokens: session_tokens(),
        return_to: "/files?folder=1".to_owned(),
    })
}

fn link_cookies(session: &str) -> String {
    format!(
        "{SESSION_COOKIE}={session}; {}",
        binding_cookie_header("bind-ING_1")
    )
}

async fn link_callback(stack: &Stack, session: &str) -> crate::worker::support::Reply {
    let cookie = link_cookies(session);
    stack
        .call(
            "GET",
            &format!("{CALLBACK}?code=4%2F0A&state=s-1"),
            &[("cookie", cookie.as_str())],
            "",
        )
        .await
}

#[tokio::test]
async fn a_callback_with_a_live_session_cookie_completes_the_link_as_that_account_and_lands_on_the_account_page()
 {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(COMPLETE_LINK, Ok::<_, SignInError>(Linked));

    // Act
    let answer = link_callback(&stack, MEMBER_TOKEN).await;

    // Assert
    assert_eq!(answer.status, 302);
    assert_eq!(answer.header("location"), Some("/account?linked=1"));
    assert!(answer.header("set-cookie").is_none());
    assert_eq!(
        stack.log(COMPLETE_LINK),
        [format!(
            "{COMPLETE_LINK} {MEMBER_ACCOUNT} provider=github params=code=4/0A&state=s-1 \
             binding=bind-ING_1 host=sunnycloud.test"
        )]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_link_refusal_lands_on_the_account_page_with_the_reason_and_sets_no_cookie() {
    // Arrange
    let stack = Stack::start().await;
    let refusals = [
        SignInError::IdentityTaken,
        SignInError::TrustedFailure {
            code: ErrorCode::AccountNotActive,
        },
        SignInError::InvitationRequired,
    ];
    for refusal in refusals {
        stack.script.push(COMPLETE_LINK, Err::<Linked, _>(refusal));
    }

    // Act
    let mut answers = Vec::new();
    for _ in 0..3 {
        answers.push(link_callback(&stack, MEMBER_TOKEN).await);
    }

    // Assert
    let landings: Vec<_> = answers
        .iter()
        .map(|answer| (answer.status, answer.header("location").map(str::to_owned)))
        .collect();
    assert_eq!(
        landings,
        [
            (302, Some("/account?error=identity_taken".to_owned())),
            (302, Some("/account?error=account_not_active".to_owned())),
            (302, Some("/account?error=invitation_required".to_owned())),
        ]
    );
    assert!(
        answers
            .iter()
            .all(|answer| answer.header("set-cookie").is_none())
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_session_cookie_beside_a_sign_in_state_still_finishes_the_sign_in() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(COMPLETE_LINK, Err::<Linked, _>(SignInError::UntrustedState));
    stack.script.push(COMPLETE, session_outcome());

    // Act
    let answer = link_callback(&stack, MEMBER_TOKEN).await;

    // Assert
    assert_eq!(answer.status, 302);
    assert_eq!(answer.header("location"), Some("/files?folder=1"));
    let sign_ins = stack
        .log(COMPLETE)
        .into_iter()
        .filter(|line| !line.starts_with(COMPLETE_LINK))
        .count();
    assert_eq!((stack.log(COMPLETE_LINK).len(), sign_ins), (1, 1));
    stack.stop().await;
}

#[tokio::test]
async fn a_link_refusal_with_no_redirect_reason_keeps_the_ports_answer() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        COMPLETE_LINK,
        Err::<Linked, _>(SignInError::ProviderUnavailable),
    );

    // Act
    let answer = link_callback(&stack, MEMBER_TOKEN).await;

    // Assert
    assert_eq!(answer.status, 503);
    stack.stop().await;
}

#[tokio::test]
async fn a_session_cookie_that_01_does_not_accept_leaves_the_callback_a_plain_sign_in() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(COMPLETE, session_outcome());

    // Act
    let answer = link_callback(&stack, "stale-session").await;

    // Assert
    assert_eq!(answer.status, 302);
    assert_eq!(answer.header("location"), Some("/files?folder=1"));
    assert!(stack.log(COMPLETE_LINK).is_empty());
    assert_eq!(stack.log(COMPLETE).len(), 1);
    stack.stop().await;
}

#[tokio::test]
async fn a_failing_session_check_is_the_ports_answer_and_reaches_neither_completion() {
    // Arrange
    let stack = Stack::start().await;

    // Act
    let answer = link_callback(&stack, FAULT_TOKEN).await;

    // Assert
    assert_eq!(answer.status, 500);
    assert_eq!(stack.calls(), 0);
    stack.stop().await;
}
