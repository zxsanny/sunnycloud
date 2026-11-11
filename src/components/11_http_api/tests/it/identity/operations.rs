//! AC-1: every operation of the groups, called once with valid input and the matching credential, reaches exactly
//! one port method, with the account of the principal.

use contracts::{
    AccountServiceError, AccountView, AdminError, AdminUserView, DeletionAccepted, DeletionError,
    DeviceGrantError, DeviceInfo, DeviceToken, HealthItem, HealthReport, IdentityView, InviteError,
    InviteInfo, InviteToken, Role, SessionError, SessionInfo, SignInError, SignInOutcome,
};
use sc_ids::TokenKind;
use serde_json::json;
use uuid::Uuid;

use super::support::*;
use crate::auth::support::{ADMIN_ACCOUNT, ADMIN_TOKEN, DEVICE_ACCOUNT, DEVICE_TOKEN};
use crate::auth::support::{MEMBER_ACCOUNT, MEMBER_TOKEN};
use crate::harness::Script;

#[derive(Clone, Copy)]
enum Caller {
    Anonymous,
    Member,
    Admin,
    Device,
}

impl Caller {
    fn account(self) -> Option<Uuid> {
        match self {
            Caller::Anonymous => None,
            Caller::Member => Some(MEMBER_ACCOUNT),
            Caller::Admin => Some(ADMIN_ACCOUNT),
            Caller::Device => Some(DEVICE_ACCOUNT),
        }
    }

    fn token(self) -> Option<&'static str> {
        match self {
            Caller::Anonymous => None,
            Caller::Member => Some(MEMBER_TOKEN),
            Caller::Admin => Some(ADMIN_TOKEN),
            Caller::Device => Some(DEVICE_TOKEN),
        }
    }
}

struct Operation {
    caller: Caller,
    method: &'static str,
    path: String,
    body: String,
    status: u16,
    /// The port method the one call must reach.
    port: &'static str,
    script: fn(&Script),
}

fn op(
    caller: Caller,
    method: &'static str,
    path: &str,
    body: &str,
    status: u16,
    port: &'static str,
    script: fn(&Script),
) -> Operation {
    Operation {
        caller,
        method,
        path: path.to_owned(),
        body: body.to_owned(),
        status,
        port,
        script,
    }
}

fn account_view() -> AccountView {
    AccountView {
        id: MEMBER_ACCOUNT,
        display_name: "Ada".to_owned(),
        role: Role::User,
        identities: vec![IdentityView {
            provider: "github".to_owned(),
            email: None,
        }],
        quota: quota(),
        root_node_id: TARGET,
    }
}

fn operations() -> Vec<Operation> {
    use Caller::*;
    let login = json!({"provider": "github", "return_to": "/f", "client_kind": "web"}).to_string();
    let exchange = json!({"code": "c", "code_verifier": "v"}).to_string();
    let grant = json!({"device_name": "d", "platform": "linux"}).to_string();
    vec![
        op(
            Anonymous,
            "GET",
            "/api/v1/auth/providers",
            "",
            200,
            "SignInService::list_providers",
            |s| {
                s.push(
                    "SignInService::list_providers",
                    Ok::<_, SignInError>(providers()),
                )
            },
        ),
        op(
            Anonymous,
            "POST",
            "/api/v1/auth/login-attempts",
            &login,
            201,
            "SignInService::begin",
            |s| {
                s.push(
                    "SignInService::begin",
                    Ok::<_, SignInError>(redirect_to_provider()),
                )
            },
        ),
        op(
            Anonymous,
            "GET",
            "/api/v1/auth/providers/github/callback",
            "",
            302,
            "SignInService::complete",
            |s| {
                s.push(
                    "SignInService::complete",
                    Ok::<_, SignInError>(SignInOutcome::Session {
                        tokens: session_tokens(),
                        return_to: "/f".to_owned(),
                    }),
                )
            },
        ),
        op(
            Anonymous,
            "POST",
            "/api/v1/auth/mobile-code-exchanges",
            &exchange,
            200,
            "SignInService::exchange_mobile_code",
            |s| {
                s.push(
                    "SignInService::exchange_mobile_code",
                    Ok::<_, SignInError>(session_tokens()),
                )
            },
        ),
        op(
            Anonymous,
            "POST",
            "/api/v1/auth/refresh-tokens",
            r#"{"refresh_token":"r"}"#,
            200,
            "SessionService::refresh",
            |s| {
                s.push(
                    "SessionService::refresh",
                    Ok::<_, SessionError>(session_tokens()),
                )
            },
        ),
        op(
            Member,
            "GET",
            "/api/v1/sessions",
            "",
            200,
            "SessionService::list",
            |s| {
                s.push(
                    "SessionService::list",
                    Ok::<_, SessionError>(Vec::<SessionInfo>::new()),
                )
            },
        ),
        op(
            Member,
            "DELETE",
            &format!("/api/v1/sessions/{OTHER}"),
            "",
            204,
            "SessionService::revoke",
            |s| s.push("SessionService::revoke", Ok::<(), SessionError>(())),
        ),
        op(
            Member,
            "DELETE",
            "/api/v1/sessions/current",
            "",
            204,
            "SessionService::revoke",
            |s| s.push("SessionService::revoke", Ok::<(), SessionError>(())),
        ),
        op(
            Member,
            "POST",
            "/api/v1/identity-link-attempts",
            r#"{"provider":"github"}"#,
            200,
            "SignInService::begin_link",
            |s| {
                s.push(
                    "SignInService::begin_link",
                    Ok::<_, SignInError>(redirect_to_provider()),
                )
            },
        ),
        op(
            Anonymous,
            "POST",
            "/api/v1/device-grants",
            &grant,
            200,
            "DeviceGrantService::request",
            |s| {
                s.push(
                    "DeviceGrantService::request",
                    Ok::<_, DeviceGrantError>(device_grant()),
                )
            },
        ),
        op(
            Anonymous,
            "POST",
            "/api/v1/device-tokens",
            r#"{"device_code":"d"}"#,
            200,
            "DeviceGrantService::poll",
            |s| {
                s.push(
                    "DeviceGrantService::poll",
                    Ok::<_, DeviceGrantError>(DeviceToken(token(TokenKind::DeviceToken, "dev-T"))),
                )
            },
        ),
        op(
            Member,
            "PATCH",
            &format!("/api/v1/device-grants/{GRANT}"),
            r#"{"decision":"deny"}"#,
            204,
            "DeviceGrantService::decide",
            |s| s.push("DeviceGrantService::decide", Ok::<(), DeviceGrantError>(())),
        ),
        op(
            Member,
            "GET",
            "/api/v1/devices",
            "",
            200,
            "DeviceGrantService::list_devices",
            |s| {
                s.push(
                    "DeviceGrantService::list_devices",
                    Ok::<_, DeviceGrantError>(Vec::<DeviceInfo>::new()),
                )
            },
        ),
        op(
            Member,
            "DELETE",
            &format!("/api/v1/devices/{OTHER}"),
            "",
            204,
            "DeviceGrantService::revoke_device",
            |s| {
                s.push(
                    "DeviceGrantService::revoke_device",
                    Ok::<(), DeviceGrantError>(()),
                )
            },
        ),
        op(
            Device,
            "DELETE",
            "/api/v1/devices/me",
            "",
            204,
            "DeviceGrantService::revoke_self",
            |s| {
                s.push(
                    "DeviceGrantService::revoke_self",
                    Ok::<(), DeviceGrantError>(()),
                )
            },
        ),
        op(
            Member,
            "GET",
            "/api/v1/accounts/me",
            "",
            200,
            "AccountService::view",
            |s| {
                s.push(
                    "AccountService::view",
                    Ok::<_, AccountServiceError>(account_view()),
                )
            },
        ),
        op(
            Member,
            "DELETE",
            "/api/v1/accounts/me",
            r#"{"confirmation":"ada"}"#,
            202,
            "AccountDeletionService::request",
            |s| {
                s.push(
                    "AccountDeletionService::request",
                    Ok::<_, DeletionError>(DeletionAccepted { deadline: WHEN }),
                )
            },
        ),
        op(
            Device,
            "GET",
            "/api/v1/accounts/me/quota",
            "",
            200,
            "AccountService::quota_view",
            |s| {
                s.push(
                    "AccountService::quota_view",
                    Ok::<_, AccountServiceError>(quota()),
                )
            },
        ),
        op(
            Admin,
            "GET",
            "/api/v1/admin/users",
            "",
            200,
            "AdminService::list_users",
            |s| {
                s.push(
                    "AdminService::list_users",
                    Ok::<_, AdminError>(Vec::<AdminUserView>::new()),
                )
            },
        ),
        op(
            Admin,
            "PATCH",
            &format!("/api/v1/admin/users/{TARGET}"),
            r#"{"state":"active"}"#,
            204,
            "AdminService::update_user",
            |s| s.push("AdminService::update_user", Ok::<(), AdminError>(())),
        ),
        op(
            Admin,
            "PUT",
            &format!("/api/v1/admin/users/{TARGET}/quota"),
            r#"{"limit":9}"#,
            204,
            "AdminService::set_quota",
            |s| s.push("AdminService::set_quota", Ok::<(), AdminError>(())),
        ),
        op(
            Admin,
            "GET",
            "/api/v1/admin/health",
            "",
            200,
            "AdminService::health",
            |s| {
                s.push(
                    "AdminService::health",
                    HealthReport {
                        items: vec![("database".to_owned(), HealthItem::ok())],
                    },
                )
            },
        ),
        op(
            Admin,
            "POST",
            "/api/v1/admin/invites",
            r#"{"note":"n"}"#,
            200,
            "InviteService::create_invite",
            |s| {
                s.push(
                    "InviteService::create_invite",
                    Ok::<_, InviteError>(InviteToken(token(TokenKind::Invite, "inv-T"))),
                )
            },
        ),
        op(
            Admin,
            "GET",
            "/api/v1/admin/invites",
            "",
            200,
            "InviteService::list",
            |s| {
                s.push(
                    "InviteService::list",
                    Ok::<_, InviteError>(Vec::<InviteInfo>::new()),
                )
            },
        ),
        op(
            Admin,
            "DELETE",
            &format!("/api/v1/admin/invites/{INVITE}"),
            "",
            204,
            "InviteService::revoke",
            |s| s.push("InviteService::revoke", Ok::<(), InviteError>(())),
        ),
    ]
}

#[tokio::test]
async fn every_operation_makes_one_call_to_its_port_with_the_account_of_the_principal() {
    // Arrange
    let stack = Stack::start().await;
    let operations = operations();

    // Act
    let mut outcomes = Vec::new();
    for operation in &operations {
        (operation.script)(&stack.script);
        let before = stack.calls();
        let answer = match operation.caller.token() {
            Some(token) => {
                stack
                    .with_token(token, operation.method, &operation.path, &operation.body)
                    .await
            }
            None => {
                stack
                    .anonymous(operation.method, &operation.path, &operation.body)
                    .await
            }
        };
        let calls: Vec<String> = stack.script.log().split_off(before);
        outcomes.push((operation, answer.status, calls));
    }

    // Assert
    assert_eq!(operations.len(), 25);
    for (operation, status, calls) in &outcomes {
        let label = format!("{} {}", operation.method, operation.path);
        assert_eq!(*status, operation.status, "{label}");
        assert_eq!(calls.len(), 1, "{label}: {calls:?}");
        assert!(calls[0].starts_with(operation.port), "{label}: {calls:?}");
        if let Some(account) = operation.caller.account() {
            assert!(
                calls[0].contains(&account.to_string()),
                "{label}: {calls:?}"
            );
        }
    }
    stack.stop().await;
}
