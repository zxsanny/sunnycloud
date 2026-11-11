//! The rows of the identity, account and admin operations in the registry: class, body class, state-changing
//! flag, owner port method, success status and the ID parameters declared.

use sunnycloud_http_api::registry::{
    AuthClass, BodyClass, HttpMethod, ParamLocation, RouteSpec, document, production_routes,
    validate,
};

use AuthClass::{Admin, DeviceOnly, Public, SessionOnly, SessionOrDevice};
use BodyClass::{Auth, Small};
use HttpMethod::{Delete, Get, Patch, Post, Put};

/// (method, path, auth, body, state-changing, owner, success)
type Row = (
    HttpMethod,
    &'static str,
    AuthClass,
    BodyClass,
    bool,
    &'static str,
    u16,
);

const NONE: BodyClass = BodyClass::None;

const ROWS: [Row; 25] = [
    (
        Get,
        "/api/v1/auth/providers",
        Public,
        NONE,
        false,
        "SignInService::list_providers",
        200,
    ),
    (
        Post,
        "/api/v1/auth/login-attempts",
        Public,
        Auth,
        true,
        "SignInService::begin",
        201,
    ),
    (
        Get,
        "/api/v1/auth/providers/{provider}/callback",
        Public,
        NONE,
        true,
        "SignInService::complete",
        302,
    ),
    (
        Post,
        "/api/v1/auth/mobile-code-exchanges",
        Public,
        Auth,
        true,
        "SignInService::exchange_mobile_code",
        200,
    ),
    (
        Post,
        "/api/v1/auth/refresh-tokens",
        Public,
        Auth,
        true,
        "SessionService::refresh",
        200,
    ),
    (
        Get,
        "/api/v1/sessions",
        SessionOnly,
        NONE,
        false,
        "SessionService::list",
        200,
    ),
    (
        Delete,
        "/api/v1/sessions/{id}",
        SessionOnly,
        NONE,
        true,
        "SessionService::revoke",
        204,
    ),
    (
        Delete,
        "/api/v1/sessions/current",
        SessionOnly,
        NONE,
        true,
        "SessionService::revoke",
        204,
    ),
    (
        Post,
        "/api/v1/identity-link-attempts",
        SessionOnly,
        Small,
        true,
        "SignInService::begin_link",
        200,
    ),
    (
        Post,
        "/api/v1/device-grants",
        Public,
        Auth,
        true,
        "DeviceGrantService::request",
        200,
    ),
    (
        Post,
        "/api/v1/device-tokens",
        Public,
        Auth,
        true,
        "DeviceGrantService::poll",
        200,
    ),
    (
        Patch,
        "/api/v1/device-grants/{id}",
        SessionOnly,
        Small,
        true,
        "DeviceGrantService::decide",
        204,
    ),
    (
        Get,
        "/api/v1/devices",
        SessionOnly,
        NONE,
        false,
        "DeviceGrantService::list_devices",
        200,
    ),
    (
        Delete,
        "/api/v1/devices/{id}",
        SessionOnly,
        NONE,
        true,
        "DeviceGrantService::revoke_device",
        204,
    ),
    (
        Delete,
        "/api/v1/devices/me",
        DeviceOnly,
        NONE,
        true,
        "DeviceGrantService::revoke_self",
        204,
    ),
    (
        Get,
        "/api/v1/accounts/me",
        SessionOrDevice,
        NONE,
        false,
        "AccountService::view",
        200,
    ),
    (
        Delete,
        "/api/v1/accounts/me",
        SessionOnly,
        Small,
        true,
        "AccountDeletionService::request",
        202,
    ),
    (
        Get,
        "/api/v1/accounts/me/quota",
        SessionOrDevice,
        NONE,
        false,
        "AccountService::quota_view",
        200,
    ),
    (
        Get,
        "/api/v1/admin/users",
        Admin,
        NONE,
        false,
        "AdminService::list_users",
        200,
    ),
    (
        Patch,
        "/api/v1/admin/users/{id}",
        Admin,
        Small,
        true,
        "AdminService::update_user",
        204,
    ),
    (
        Put,
        "/api/v1/admin/users/{id}/quota",
        Admin,
        Small,
        true,
        "AdminService::set_quota",
        204,
    ),
    (
        Get,
        "/api/v1/admin/health",
        Admin,
        NONE,
        false,
        "AdminService::health",
        200,
    ),
    (
        Post,
        "/api/v1/admin/invites",
        Admin,
        Small,
        true,
        "InviteService::create_invite",
        200,
    ),
    (
        Get,
        "/api/v1/admin/invites",
        Admin,
        NONE,
        false,
        "InviteService::list",
        200,
    ),
    (
        Delete,
        "/api/v1/admin/invites/{id}",
        Admin,
        NONE,
        true,
        "InviteService::revoke",
        204,
    ),
];

fn spec(method: HttpMethod, path: &str) -> RouteSpec {
    production_routes()
        .into_iter()
        .map(|def| def.spec)
        .find(|spec| spec.method == method && spec.path == path)
        .unwrap_or_else(|| panic!("{} {path} is registered", method.key()))
}

#[test]
fn every_operation_declares_its_class_body_flag_owner_and_success() {
    // Arrange
    let rows = ROWS;

    // Act
    let found: Vec<Row> = rows
        .iter()
        .map(|(method, path, ..)| {
            let spec = spec(*method, path);
            (
                spec.method,
                *path,
                spec.auth,
                spec.body,
                spec.state_changing,
                spec.owner,
                spec.success,
            )
        })
        .collect();

    // Assert
    assert_eq!(found, rows);
}

#[test]
fn the_registry_with_these_groups_validates() {
    // Arrange
    let routes = production_routes();

    // Act
    let outcome = validate(&routes);

    // Assert
    assert_eq!(outcome, Ok(()));
}

#[test]
fn every_id_in_a_path_is_declared_and_no_operation_takes_an_account_id() {
    // Arrange
    let specs: Vec<RouteSpec> = ROWS
        .iter()
        .map(|(method, path, ..)| spec(*method, path))
        .collect();

    // Act
    let account_params: Vec<_> = specs
        .iter()
        .flat_map(|spec| spec.params.iter().map(move |param| (spec.path, param)))
        .filter(|(_, param)| param.name.contains("account"))
        .map(|(path, param)| (path, param.name))
        .collect();
    let declared: Vec<_> = specs
        .iter()
        .filter(|spec| spec.path.contains("{id}"))
        .map(|spec| {
            spec.params.iter().any(|param| {
                param.name == "id" && param.id && param.location == ParamLocation::Path
            })
        })
        .collect();

    // Assert
    assert!(account_params.is_empty(), "{account_params:?}");
    assert!(declared.iter().all(|declared| *declared));
}

#[test]
fn the_document_lists_the_operations_with_their_auth_class_and_the_callback_as_the_one_get_that_mutates()
 {
    // Arrange
    let parsed: serde_json::Value =
        serde_json::from_slice(document(&production_routes()).as_bytes()).expect("document");

    // Act
    let callback = &parsed["paths"]["/api/v1/auth/providers/{provider}/callback"]["get"];
    let own_device = &parsed["paths"]["/api/v1/devices/me"]["delete"];

    // Assert
    assert_eq!(callback["x-state-changing"], true);
    assert!(callback["responses"]["302"].is_object());
    assert_eq!(own_device["x-auth-class"], "DeviceOnly");
}
