//! Identity scope declarations (E-15/c13): every identity-owned operation, and the four AC-57 operations
//! owned elsewhere, declares in the generated OpenAPI document the auth class 01's scope model assigns.

use std::collections::HashSet;

use contracts::Scope;
use serde_json::Value;
use sunnycloud_http_api::registry::{AuthClass, document, production_routes};

/// 01's scope per owner port method, written from 01 description §3 (01 exposes no per-operation table).
/// Operations are matched by `x-owner-method`, never by path: 01 §3 paths are indicative.
/// `DeviceGrantService::revoke_self` is checked separately ([`DEVICE_ONLY`]).
const IDENTITY_SCOPES: [(&str, Scope); 17] = [
    ("SignInService::list_providers", Scope::Public),
    ("SignInService::begin", Scope::Public),
    ("SignInService::complete", Scope::Public),
    ("SignInService::exchange_mobile_code", Scope::Public),
    ("SessionService::refresh", Scope::Public),
    ("DeviceGrantService::request", Scope::Public),
    ("DeviceGrantService::poll", Scope::Public),
    ("SessionService::list", Scope::SessionOnly),
    ("SessionService::revoke", Scope::SessionOnly),
    ("SignInService::begin_link", Scope::SessionOnly),
    (
        "DeviceGrantService::lookup_for_approval",
        Scope::SessionOnly,
    ),
    ("DeviceGrantService::decide", Scope::SessionOnly),
    ("DeviceGrantService::list_devices", Scope::SessionOnly),
    ("DeviceGrantService::revoke_device", Scope::SessionOnly),
    ("InviteService::create_invite", Scope::Admin),
    ("InviteService::list", Scope::Admin),
    ("InviteService::revoke", Scope::Admin),
];

/// The AC-57 operations owned by 06 (create-share) and 07 (delete-account).
const AC57_ELSEWHERE: [(&str, Scope); 2] = [
    ("ShareService::create", Scope::SessionOnly),
    ("AccountDeletionService::request", Scope::SessionOnly),
];

/// Approve-device, create-share, delete-account and link-provider (AC-57).
const AC57: [&str; 4] = [
    "DeviceGrantService::decide",
    "ShareService::create",
    "AccountDeletionService::request",
    "SignInService::begin_link",
];

fn class_of(scope: Scope) -> AuthClass {
    match scope {
        Scope::Public => AuthClass::Public,
        Scope::SessionOnly => AuthClass::SessionOnly,
        Scope::SessionOrDevice => AuthClass::SessionOrDevice,
        Scope::Admin => AuthClass::Admin,
    }
}

/// `DELETE /devices/me` is device-only (owner decision 2026-11-12). 01's `Scope` has no device-only value, so
/// the expected class is 11's own.
const DEVICE_ONLY: [&str; 1] = ["DeviceGrantService::revoke_self"];

fn expected_classes() -> Vec<(&'static str, AuthClass)> {
    IDENTITY_SCOPES
        .iter()
        .chain(AC57_ELSEWHERE.iter())
        .map(|&(owner, scope)| (owner, class_of(scope)))
        .chain(
            DEVICE_ONLY
                .iter()
                .map(|&owner| (owner, AuthClass::DeviceOnly)),
        )
        .collect()
}

/// Every operation of `openapi` whose owner is in `expected` must declare exactly its class, and every owner in
/// `expected` must own at least one operation. Each problem names the operation and the expected class.
fn check_scopes(openapi: &Value, expected: &[(&str, AuthClass)]) -> Result<(), Vec<String>> {
    let Some(paths) = openapi["paths"].as_object() else {
        return Err(vec!["the document has no paths".to_owned()]);
    };
    let mut problems = Vec::new();
    let mut seen = HashSet::new();
    for (path, item) in paths {
        let Some(operations) = item.as_object() else {
            continue;
        };
        for (method, operation) in operations {
            let Some(owner) = operation["x-owner-method"].as_str() else {
                continue;
            };
            let Some(&(_, class)) = expected.iter().find(|(name, _)| *name == owner) else {
                continue;
            };
            seen.insert(owner);
            let declared = operation["x-auth-class"].as_str();
            if declared != Some(class.name()) {
                problems.push(format!(
                    "{} {path} ({owner}) declares {}, expected {}",
                    method.to_uppercase(),
                    declared.unwrap_or("no auth class"),
                    class.name()
                ));
            }
        }
    }
    for &(owner, class) in expected {
        if !seen.contains(owner) {
            problems.push(format!(
                "no operation owned by {owner} in the document, expected {}",
                class.name()
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

fn release_document() -> Value {
    serde_json::from_slice(document(&production_routes()).as_bytes()).expect("document")
}

fn operations_owned_by<'a>(openapi: &'a mut Value, owner: &str) -> Vec<&'a mut Value> {
    openapi["paths"]
        .as_object_mut()
        .expect("paths")
        .values_mut()
        .flat_map(|item| item.as_object_mut().expect("path item").values_mut())
        .filter(|operation| operation["x-owner-method"] == owner)
        .collect()
}

#[test]
fn every_identity_operation_declares_the_class_01_assigns() {
    // Arrange
    let openapi = release_document();

    // Act
    let outcome = check_scopes(&openapi, &expected_classes());

    // Assert
    assert_eq!(outcome, Ok(()));
}

#[test]
fn the_ac57_operations_are_session_only_so_a_device_token_cannot_pass() {
    // Arrange
    let mut openapi = release_document();
    let expected = expected_classes();

    for owner in AC57 {
        // Act
        let table = expected.iter().find(|(name, _)| *name == owner);
        let operations = operations_owned_by(&mut openapi, owner);

        // Assert
        assert_eq!(table, Some(&(owner, AuthClass::SessionOnly)), "{owner}");
        assert_eq!(operations.len(), 1, "{owner}");
        for operation in operations {
            assert_eq!(operation["x-auth-class"], "SessionOnly", "{owner}");
            let schemes: Vec<&str> = operation["security"]
                .as_array()
                .expect("security")
                .iter()
                .flat_map(|requirement| requirement.as_object().expect("requirement").keys())
                .map(String::as_str)
                .collect();
            assert!(!schemes.is_empty(), "{owner} needs a credential");
            assert!(!schemes.contains(&"bearerDevice"), "{owner}: {schemes:?}");
        }
    }
    assert!(!AuthClass::SessionOnly.loosens(AuthClass::DeviceOnly));
}

#[test]
fn drift_fails_naming_each_drifted_operation_and_its_expected_class() {
    // Arrange
    let mut openapi = release_document();
    for operation in operations_owned_by(&mut openapi, "DeviceGrantService::decide") {
        operation["x-auth-class"] = Value::from("SessionOrDevice");
    }
    for operation in operations_owned_by(&mut openapi, "InviteService::create_invite") {
        operation
            .as_object_mut()
            .expect("operation")
            .remove("x-auth-class");
    }

    // Act
    let mut problems = check_scopes(&openapi, &expected_classes()).expect_err("drift is refused");

    // Assert
    problems.sort();
    assert_eq!(
        problems,
        [
            "PATCH /api/v1/device-grants/{id} (DeviceGrantService::decide) declares SessionOrDevice, \
             expected SessionOnly",
            "POST /api/v1/admin/invites (InviteService::create_invite) declares no auth class, \
             expected Admin",
        ],
    );
}

#[test]
fn an_expected_operation_missing_from_the_document_fails() {
    // Arrange
    let mut openapi = release_document();
    for operation in operations_owned_by(&mut openapi, "SignInService::begin_link") {
        operation["x-owner-method"] = Value::from("SignInService::renamed");
    }

    // Act
    let problems = check_scopes(&openapi, &expected_classes()).expect_err("a gap is refused");

    // Assert
    assert_eq!(
        problems,
        ["no operation owned by SignInService::begin_link in the document, expected SessionOnly"],
    );
}
