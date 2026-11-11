//! No token, code, return path or email reaches a log line or a metric label, on the success path or the failing
//! one (the ports of 01 are the only places that see them).

use std::sync::{Arc, Mutex};

use contracts::{
    AccountServiceError, AccountView, DeviceGrantError, DeviceToken, IdentityView, InviteError,
    InviteToken, Role, SessionError, SignInError, SignInOutcome,
};
use sc_clock::SystemClock;
use sc_ids::TokenKind;
use sc_observability::logging::Level;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use serde_json::json;

use super::support::*;
use crate::auth::support::ADMIN_TOKEN;

const PROVIDER_CODE: &str = "PROVIDER_CODE_9f3a";
const RETURN_PATH: &str = "/private-return-path-7c1d";
const MOBILE_CODE: &str = "mobile-code-4b2e";
const INVITE_PLAIN: &str = "invite-plain-5d8f";
const DEVICE_PLAIN: &str = "device-plain-6a0c";
const EMAIL: &str = "ada.secret@example.test";

#[derive(Default)]
struct Lines(Mutex<Vec<String>>);

impl LineSink for Lines {
    fn write_line(&self, line: &str) -> std::io::Result<()> {
        self.0
            .lock()
            .expect("the sink is not poisoned")
            .push(line.to_owned());
        Ok(())
    }
}

#[tokio::test]
async fn no_secret_of_a_sign_in_a_device_grant_or_an_invite_reaches_a_log_line_or_a_metric() {
    // Arrange
    let lines = Arc::new(Lines::default());
    let (logger, _stats) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::INFO,
            component: Component::Api,
        },
        Arc::new(SystemClock::new()),
        lines.clone(),
    );
    // A second registered dispatcher makes tracing ask each thread's own default (see `metrics_log.rs`).
    let _anchor = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let _logging = tracing::subscriber::set_default(logger);
    let stack = Stack::start().await;
    stack.script.push(
        "SignInService::complete",
        Ok::<_, SignInError>(SignInOutcome::MobileCode {
            code: token(TokenKind::MobileLoginCode, MOBILE_CODE),
            return_to: RETURN_PATH.to_owned(),
        }),
    );
    stack.script.push(
        "SignInService::complete",
        Err::<SignInOutcome, SignInError>(SignInError::UntrustedState),
    );
    stack.script.push(
        "SessionService::refresh",
        Err::<contracts::SessionTokens, SessionError>(SessionError::ReuseDetected),
    );
    stack.script.push(
        "DeviceGrantService::poll",
        Ok::<_, DeviceGrantError>(DeviceToken(token(TokenKind::DeviceToken, DEVICE_PLAIN))),
    );
    stack.script.push(
        "InviteService::create_invite",
        Ok::<_, InviteError>(InviteToken(token(TokenKind::Invite, INVITE_PLAIN))),
    );
    stack.script.push(
        "AccountService::view",
        Ok::<_, AccountServiceError>(AccountView {
            id: TARGET,
            display_name: "Ada".to_owned(),
            role: Role::User,
            identities: vec![IdentityView {
                provider: "github".to_owned(),
                email: Some(EMAIL.to_owned()),
            }],
            quota: quota(),
            root_node_id: TARGET,
        }),
    );
    let callback = format!("/api/v1/auth/providers/github/callback?code={PROVIDER_CODE}&state=s");
    let login = json!({
        "provider": "github", "return_to": RETURN_PATH, "client_kind": "web",
        "invite_token": INVITE_PLAIN,
    })
    .to_string();

    // Act
    let answers = [
        stack.anonymous("GET", &callback, "").await,
        stack.anonymous("GET", &callback, "").await,
        stack
            .anonymous("POST", "/api/v1/auth/login-attempts", &login)
            .await,
        stack
            .anonymous(
                "POST",
                "/api/v1/auth/refresh-tokens",
                &json!({"refresh_token": MOBILE_CODE}).to_string(),
            )
            .await,
        stack
            .anonymous(
                "POST",
                "/api/v1/device-tokens",
                &json!({"device_code": DEVICE_PLAIN}).to_string(),
            )
            .await,
        stack
            .with_token(
                ADMIN_TOKEN,
                "POST",
                "/api/v1/admin/invites",
                r#"{"note":"n"}"#,
            )
            .await,
        stack.as_member("GET", "/api/v1/accounts/me", "").await,
    ];
    let scrape = stack.metrics.render().await.expect("render metrics");
    let log = lines.0.lock().expect("the sink is not poisoned").clone();

    // Assert
    let statuses: Vec<u16> = answers.iter().map(|answer| answer.status).collect();
    assert_eq!(statuses, [302, 302, 500, 401, 200, 200, 200]);
    assert!(
        log.len() >= 5,
        "the redirects and failures are logged: {log:?}"
    );
    for secret in [
        PROVIDER_CODE,
        RETURN_PATH,
        MOBILE_CODE,
        INVITE_PLAIN,
        DEVICE_PLAIN,
        EMAIL,
    ] {
        assert!(!scrape.contains(secret), "a metric carries {secret}");
        for line in &log {
            assert!(
                !line.contains(secret),
                "a log line carries {secret}: {line}"
            );
        }
    }
    stack.stop().await;
}
