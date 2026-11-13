use contracts::{
    AccountStatus, AdminError, AdminUserView, HealthItem, HealthReport, HealthStatus, InviteError,
    InviteInfo, InviteToken, Role,
};
use sc_ids::TokenKind;
use serde_json::json;

use super::support::*;
use crate::auth::support::{ADMIN_ACCOUNT, ADMIN_TOKEN};

const LIST_USERS: &str = "AdminService::list_users";
const UPDATE: &str = "AdminService::update_user";
const QUOTA: &str = "AdminService::set_quota";
const SECRET: &str = "inv-ShownOnce_plain";

fn invite_info() -> InviteInfo {
    InviteInfo {
        id: INVITE,
        note: "for Bob".to_owned(),
        created_at: WHEN,
        expires_at: WHEN,
        redeemed_at: None,
    }
}

async fn as_admin(
    stack: &Stack,
    method: &str,
    path: &str,
    body: &str,
) -> crate::worker::support::Reply {
    stack.with_token(ADMIN_TOKEN, method, path, body).await
}

#[tokio::test]
async fn the_user_list_acts_as_the_admin_and_pages_by_offset_and_limit() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        LIST_USERS,
        Ok::<_, AdminError>(vec![AdminUserView {
            account: TARGET,
            role: Role::Admin,
            state: AccountStatus::Disabled,
            last_login: Some(WHEN),
            used: 10,
            limit: None,
        }]),
    );
    stack
        .script
        .push(LIST_USERS, Ok::<_, AdminError>(Vec::<AdminUserView>::new()));

    // Act
    let listed = as_admin(&stack, "GET", "/api/v1/admin/users?offset=20&limit=5", "").await;
    let default_page = as_admin(&stack, "GET", "/api/v1/admin/users", "").await;
    let by_member = stack.as_member("GET", "/api/v1/admin/users", "").await;

    // Assert
    assert_eq!(
        listed.json(),
        json!({"items": [{
            "id": TARGET, "role": "admin", "state": "disabled", "last_login": WHEN_TEXT,
            "used": 10, "limit": null,
        }]})
    );
    assert_eq!(default_page.status, 200);
    assert_eq!(
        (by_member.status, by_member.code().as_str()),
        (403, "forbidden")
    );
    assert_eq!(
        stack.log(LIST_USERS),
        [
            format!("{LIST_USERS} admin {ADMIN_ACCOUNT} offset=20 limit=5"),
            format!("{LIST_USERS} admin {ADMIN_ACCOUNT} offset=0 limit=1000"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn an_update_passes_the_target_of_the_path_and_the_role_and_state_of_the_body() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(UPDATE, Ok::<(), AdminError>(()));
    stack.script.push(UPDATE, Ok::<(), AdminError>(()));
    let path = format!("/api/v1/admin/users/{TARGET}");

    // Act
    let both = as_admin(
        &stack,
        "PATCH",
        &path,
        r#"{"role":"user","state":"disabled"}"#,
    )
    .await;
    let none = as_admin(&stack, "PATCH", &path, "{}").await;
    let bad = as_admin(&stack, "PATCH", &path, r#"{"role":"root"}"#).await;

    // Assert
    assert_eq!((both.status, none.status, bad.status), (204, 204, 400));
    assert_eq!(
        stack.log(UPDATE),
        [
            format!("{UPDATE} admin {ADMIN_ACCOUNT} {TARGET} Some(User) Some(Disabled)"),
            format!("{UPDATE} admin {ADMIN_ACCOUNT} {TARGET} None None"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_last_admin_is_409_and_an_unknown_user_is_404() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(UPDATE, Err::<(), AdminError>(AdminError::LastAdmin));
    stack
        .script
        .push(UPDATE, Err::<(), AdminError>(AdminError::NotFound));
    let path = format!("/api/v1/admin/users/{TARGET}");

    // Act
    let last = as_admin(&stack, "PATCH", &path, r#"{"role":"user"}"#).await;
    let unknown = as_admin(&stack, "PATCH", &path, r#"{"state":"disabled"}"#).await;

    // Assert
    assert_eq!((last.status, last.code().as_str()), (409, "last_admin"));
    assert_eq!(
        (unknown.status, unknown.code().as_str()),
        (404, "not_found")
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_quota_is_a_whole_number_of_bytes_or_null_for_the_default() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(QUOTA, Ok::<(), AdminError>(()));
    stack.script.push(QUOTA, Ok::<(), AdminError>(()));
    stack
        .script
        .push(QUOTA, Err::<(), AdminError>(AdminError::InvalidLimit));
    let path = format!("/api/v1/admin/users/{TARGET}/quota");

    // Act
    let set = as_admin(&stack, "PUT", &path, r#"{"limit":5000}"#).await;
    let restored = as_admin(&stack, "PUT", &path, r#"{"limit":null}"#).await;
    let refused = as_admin(&stack, "PUT", &path, r#"{"limit":-1}"#).await;
    let fraction = as_admin(&stack, "PUT", &path, r#"{"limit":1.5}"#).await;
    let missing = as_admin(&stack, "PUT", &path, "{}").await;

    // Assert
    assert_eq!((set.status, restored.status), (204, 204));
    assert_eq!(refused.status, 400);
    assert_eq!((fraction.status, missing.status), (400, 400));
    assert_eq!(
        stack.log(QUOTA),
        [
            format!("{QUOTA} admin {ADMIN_ACCOUNT} {TARGET} Some(5000)"),
            format!("{QUOTA} admin {ADMIN_ACCOUNT} {TARGET} None"),
            format!("{QUOTA} admin {ADMIN_ACCOUNT} {TARGET} Some(-1)"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_health_report_lists_each_item_with_its_status() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        "AdminService::health",
        HealthReport {
            items: vec![
                ("database".to_owned(), HealthItem::ok()),
                (
                    "blobs".to_owned(),
                    HealthItem::new(HealthStatus::Degraded, "slow"),
                ),
            ],
        },
    );

    // Act
    let answer = as_admin(&stack, "GET", "/api/v1/admin/health", "").await;

    // Assert
    assert_eq!(
        answer.json(),
        json!({"items": [
            {"name": "database", "status": "ok", "detail": ""},
            {"name": "blobs", "status": "degraded", "detail": "slow"},
        ]})
    );
    stack.stop().await;
}

#[tokio::test]
async fn an_invite_shows_its_token_once_and_the_list_never_shows_one() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        "InviteService::create_invite",
        Ok::<_, InviteError>(InviteToken(token(TokenKind::Invite, SECRET))),
    );
    stack.script.push(
        "InviteService::list",
        Ok::<_, InviteError>(vec![invite_info()]),
    );
    stack
        .script
        .push("InviteService::revoke", Ok::<(), InviteError>(()));

    // Act
    let created = as_admin(
        &stack,
        "POST",
        "/api/v1/admin/invites",
        r#"{"note":"for Bob"}"#,
    )
    .await;
    let listed = as_admin(&stack, "GET", "/api/v1/admin/invites", "").await;
    let revoked = as_admin(
        &stack,
        "DELETE",
        &format!("/api/v1/admin/invites/{INVITE}"),
        "",
    )
    .await;

    // Assert
    assert_eq!(created.json(), json!({"token": SECRET}));
    let listing = String::from_utf8_lossy(&listed.body).into_owned();
    assert!(
        !listing.contains(SECRET) && !listing.contains("token"),
        "{listing}"
    );
    assert_eq!(
        listed.json(),
        json!({"items": [{
            "id": INVITE, "note": "for Bob", "created_at": WHEN_TEXT, "expires_at": WHEN_TEXT,
            "redeemed_at": null,
        }]})
    );
    assert_eq!(revoked.status, 204);
    assert_eq!(
        stack.log("InviteService"),
        [
            format!("InviteService::create_invite ({ADMIN_ACCOUNT}, \"for Bob\")"),
            format!("InviteService::list ({ADMIN_ACCOUNT},)"),
            format!("InviteService::revoke ({ADMIN_ACCOUNT}, {INVITE})"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn revoking_an_unknown_revoked_or_redeemed_invite_is_404() {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        "InviteService::revoke",
        Err::<(), InviteError>(InviteError::NotFound),
    );
    let path = format!("/api/v1/admin/invites/{INVITE}");

    // Act
    let reply = as_admin(&stack, "DELETE", &path, "").await;

    // Assert
    assert_eq!((reply.status, reply.code().as_str()), (404, "not_found"));
    stack.stop().await;
}

#[tokio::test]
async fn no_admin_route_is_open_to_a_member_a_device_or_no_credential() {
    // Arrange
    let stack = Stack::start().await;
    let routes = [
        ("GET", "/api/v1/admin/users".to_owned(), ""),
        ("PATCH", format!("/api/v1/admin/users/{TARGET}"), "{}"),
        (
            "PUT",
            format!("/api/v1/admin/users/{TARGET}/quota"),
            r#"{"limit":1}"#,
        ),
        ("GET", "/api/v1/admin/health".to_owned(), ""),
        (
            "POST",
            "/api/v1/admin/invites".to_owned(),
            r#"{"note":"x"}"#,
        ),
        ("GET", "/api/v1/admin/invites".to_owned(), ""),
        ("DELETE", format!("/api/v1/admin/invites/{INVITE}"), ""),
    ];

    // Act
    let mut statuses = Vec::new();
    for (method, path, body) in &routes {
        statuses.push((
            stack.as_member(method, path, body).await.status,
            stack.as_device(method, path, body).await.status,
            stack.anonymous(method, path, body).await.status,
        ));
    }

    // Assert
    assert!(
        statuses.iter().all(|row| *row == (403, 403, 401)),
        "{statuses:?}"
    );
    assert_eq!(stack.calls(), 0);
    stack.stop().await;
}
