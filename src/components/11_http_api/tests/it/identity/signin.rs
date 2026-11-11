use contracts::{
    AuthorizationRedirect, ProviderList, SessionError, SessionTokens, SignInError, SignInOutcome,
};
use sc_error::ErrorCode;
use sc_ids::TokenKind;
use serde_json::json;

use super::support::*;
use crate::auth::support::{DEVICE_TOKEN, MEMBER_ACCOUNT, MEMBER_TOKEN};

const BEGIN: &str = "SignInService::begin";
const COMPLETE: &str = "SignInService::complete";
const CALLBACK: &str = "/api/v1/auth/providers/github/callback";
const BINDING_COOKIE_LINE: &str =
    "__Host-test-binding=bind-ING_1; Secure; HttpOnly; SameSite=Lax; Path=/";

fn login_body() -> String {
    json!({
        "provider": "github",
        "return_to": "/files",
        "client_kind": "web",
        "invite_token": "inv-PLAIN_token",
    })
    .to_string()
}

fn session_outcome() -> Result<SignInOutcome, SignInError> {
    Ok(SignInOutcome::Session {
        tokens: session_tokens(),
        return_to: "/files?folder=1".to_owned(),
    })
}

#[tokio::test]
async fn a_login_attempt_passes_the_body_and_the_host_and_sets_the_binding_cookie() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(BEGIN, Ok::<_, SignInError>(redirect_to_provider()));

    // Act
    let answer = stack
        .anonymous("POST", "/api/v1/auth/login-attempts", &login_body())
        .await;

    // Assert
    assert_eq!(answer.status, 201);
    assert_eq!(
        answer.json(),
        json!({"authorization_url": "https://idp.test/authorize?state=s1"})
    );
    assert_eq!(answer.header("set-cookie"), Some(BINDING_COOKIE_LINE));
    assert_eq!(
        stack.log(BEGIN),
        [format!(
            "{BEGIN} provider=github return_to=/files kind=Web invite=inv-PLAIN_token setup=none challenge=None host=sunnycloud.test"
        )]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_mobile_login_attempt_carries_its_setup_token_and_code_challenge() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(BEGIN, Ok::<_, SignInError>(redirect_to_provider()));
    let body = json!({
        "provider": "github", "return_to": "/done", "client_kind": "android",
        "setup_token": "set-UP", "code_challenge": "chal-LENGE",
    })
    .to_string();

    // Act
    let answer = stack
        .anonymous("POST", "/api/v1/auth/login-attempts", &body)
        .await;

    // Assert
    assert_eq!(answer.status, 201);
    assert_eq!(
        stack.log(BEGIN),
        [format!(
            "{BEGIN} provider=github return_to=/done kind=Android invite=none setup=set-UP challenge=Some(\"chal-LENGE\") host=sunnycloud.test"
        )]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_login_body_that_does_not_bind_is_400_and_reaches_no_port() {
    // Arrange
    let stack = Stack::start().await;
    let bodies = [
        "not json".to_owned(),
        json!({"provider": "github", "return_to": "/f"}).to_string(),
        json!({"provider": "github", "return_to": "/f", "client_kind": "tablet"}).to_string(),
        json!({"provider": "github", "return_to": "/f", "client_kind": "web", "extra": 1})
            .to_string(),
        json!({"provider": 7, "return_to": "/f", "client_kind": "web"}).to_string(),
    ];

    // Act
    let mut answers = Vec::new();
    for body in &bodies {
        answers.push(
            stack
                .anonymous("POST", "/api/v1/auth/login-attempts", body)
                .await,
        );
    }

    // Assert
    for answer in &answers {
        assert_eq!(answer.status, 400);
        assert_eq!(answer.code(), "invalid_request");
    }
    assert_eq!(stack.calls(), 0);
    stack.stop().await;
}

#[tokio::test]
async fn the_callback_passes_provider_parameters_binding_and_host_and_redirects_to_the_return_path()
{
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(COMPLETE, session_outcome());
    let cookie = binding_cookie_header("bind-ING_1");

    // Act
    let answer = stack
        .call(
            "GET",
            &format!("{CALLBACK}?code=4%2F0A+x&state=s-1"),
            &[("cookie", cookie.as_str())],
            "",
        )
        .await;

    // Assert
    assert_eq!(answer.status, 302);
    assert_eq!(answer.header("location"), Some("/files?folder=1"));
    assert_eq!(
        stack.log(COMPLETE),
        [format!(
            "{COMPLETE} provider=github params=code=4/0A x&state=s-1 binding=bind-ING_1 host=sunnycloud.test"
        )]
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_session_cookie_is_exactly_what_the_port_result_states_and_adds_no_attribute() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(COMPLETE, session_outcome());

    // Act
    let answer = stack.anonymous("GET", CALLBACK, "").await;

    // Assert
    let cookies: Vec<&str> = answer
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect();
    assert_eq!(
        cookies,
        ["__Host-test-session=acc-ESS_token; Secure; HttpOnly; SameSite=Lax; Path=/; Max-Age=900"]
    );
    assert!(!cookies[0].to_ascii_lowercase().contains("domain"));
    stack.stop().await;
}

#[tokio::test]
async fn a_callback_without_the_binding_cookie_presents_an_empty_binding_and_leaves_the_decision_to_01()
 {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        COMPLETE,
        Err::<SignInOutcome, SignInError>(SignInError::UntrustedState),
    );

    // Act
    let answer = stack
        .anonymous("GET", &format!("{CALLBACK}?state=s-1"), "")
        .await;

    // Assert
    assert_eq!(answer.status, 302);
    assert_eq!(
        stack.log(COMPLETE),
        [format!(
            "{COMPLETE} provider=github params=state=s-1 binding= host=sunnycloud.test"
        )]
    );
    stack.stop().await;
}

#[tokio::test]
async fn an_untrusted_state_a_trusted_failure_and_a_missing_invite_redirect_to_the_spa_error_route()
{
    // Arrange
    let stack = Stack::start().await;
    let failures = [
        SignInError::UntrustedState,
        SignInError::TrustedFailure {
            code: ErrorCode::AccountNotActive,
        },
        SignInError::InvitationRequired,
    ];
    for failure in failures {
        stack
            .script
            .push(COMPLETE, Err::<SignInOutcome, SignInError>(failure));
    }

    // Act
    let mut answers = Vec::new();
    for _ in 0..3 {
        answers.push(stack.anonymous("GET", CALLBACK, "").await);
    }

    // Assert
    let landings: Vec<_> = answers
        .iter()
        .map(|answer| (answer.status, answer.header("location").map(str::to_owned)))
        .collect();
    assert_eq!(
        landings,
        [
            (302, Some("/login?error=invalid_request".to_owned())),
            (302, Some("/login?error=account_not_active".to_owned())),
            (302, Some("/login?error=invitation_required".to_owned())),
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
async fn a_mobile_outcome_redirects_with_the_one_time_code_and_sets_no_cookie() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        COMPLETE,
        Ok::<_, SignInError>(SignInOutcome::MobileCode {
            code: token(TokenKind::MobileLoginCode, "mob-CODE_1"),
            return_to: "/app/done?x=1".to_owned(),
        }),
    );

    // Act
    let answer = stack.anonymous("GET", CALLBACK, "").await;

    // Assert
    assert_eq!(answer.status, 302);
    assert_eq!(
        answer.header("location"),
        Some("/app/done?x=1&code=mob-CODE_1")
    );
    assert!(answer.header("set-cookie").is_none());
    stack.stop().await;
}

#[tokio::test]
async fn the_mobile_code_exchange_answers_tokens_in_the_body_and_an_invalid_code_is_400() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        "SignInService::exchange_mobile_code",
        Ok::<_, SignInError>(session_tokens()),
    );
    stack.script.push(
        "SignInService::exchange_mobile_code",
        Err::<SessionTokens, SignInError>(SignInError::InvalidCode),
    );
    let body = json!({"code": "mob-CODE_1", "code_verifier": "ver-IFIER"}).to_string();

    // Act
    let tokens = stack
        .anonymous("POST", "/api/v1/auth/mobile-code-exchanges", &body)
        .await;
    let invalid = stack
        .anonymous("POST", "/api/v1/auth/mobile-code-exchanges", &body)
        .await;

    // Assert
    assert_eq!(tokens.status, 200);
    assert_eq!(
        tokens.json(),
        json!({"access_token": ACCESS, "refresh_token": REFRESH, "expires_in": 900})
    );
    assert!(tokens.header("set-cookie").is_none());
    assert_eq!(invalid.status, 400);
    assert_eq!(invalid.code(), "invalid_request");
    assert_eq!(
        stack.log("SignInService::exchange_mobile_code"),
        ["SignInService::exchange_mobile_code mob-CODE_1 ver-IFIER"; 2]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_refresh_rotates_the_token_in_the_body_and_a_reused_one_is_401() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        "SessionService::refresh",
        Ok::<_, SessionError>(session_tokens()),
    );
    stack.script.push(
        "SessionService::refresh",
        Err::<SessionTokens, SessionError>(SessionError::ReuseDetected),
    );
    let body = json!({"refresh_token": "ref-OLD"}).to_string();

    // Act
    let rotated = stack
        .anonymous("POST", "/api/v1/auth/refresh-tokens", &body)
        .await;
    let reused = stack
        .anonymous("POST", "/api/v1/auth/refresh-tokens", &body)
        .await;

    // Assert
    assert_eq!(rotated.status, 200);
    assert_eq!(rotated.json()["refresh_token"], REFRESH);
    assert_eq!(reused.status, 401);
    assert_eq!(reused.code(), "unauthenticated");
    assert_eq!(
        stack.log("SessionService::refresh"),
        ["SessionService::refresh ref-OLD"; 2]
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_provider_list_carries_the_setup_flag() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        "SignInService::list_providers",
        Ok::<ProviderList, SignInError>(providers()),
    );

    // Act
    let answer = stack.anonymous("GET", "/api/v1/auth/providers", "").await;

    // Assert
    assert_eq!(answer.status, 200);
    assert_eq!(
        answer.json(),
        json!({"providers": [{"key": "github", "display_name": "GitHub"}], "setup_required": true})
    );
    stack.stop().await;
}

#[tokio::test]
async fn an_identity_link_attempt_is_for_a_session_and_sets_the_binding_cookie() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        "SignInService::begin_link",
        Ok::<AuthorizationRedirect, SignInError>(redirect_to_provider()),
    );
    let body = json!({"provider": "github"}).to_string();

    // Act
    let by_device = stack
        .with_token(
            DEVICE_TOKEN,
            "POST",
            "/api/v1/identity-link-attempts",
            &body,
        )
        .await;
    let by_member = stack
        .with_token(
            MEMBER_TOKEN,
            "POST",
            "/api/v1/identity-link-attempts",
            &body,
        )
        .await;

    // Assert
    assert_eq!(by_device.status, 403);
    assert_eq!(by_member.status, 200);
    assert_eq!(by_member.header("set-cookie"), Some(BINDING_COOKIE_LINE));
    assert_eq!(
        stack.log("SignInService::begin_link"),
        [format!("SignInService::begin_link {MEMBER_ACCOUNT} github")]
    );
    stack.stop().await;
}
