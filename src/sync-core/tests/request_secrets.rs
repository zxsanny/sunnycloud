//! AC-7 of AZ-3967: tokens appear in the `Authorization` header only, never in URLs, error texts or logs.

mod request_support;
mod support;

use std::sync::Arc;

use request_support::{
    ACCESS, FixtureTransport, REFRESH, SentinelCredentials, capture_logs, clock, drive,
};
use sc_config::Secret;
use sc_error::ErrorCode;
use serde_json::json;
use support::{Answer, Fault, Fixture, Route, Script};
use sync_core::{Credential, Mapped, Method, Request, RequestPath, SyncError};
use sync_core_proto::RetryPolicy;

fn sentinels_in(text: &str) -> usize {
    text.matches(ACCESS).count() + text.matches(REFRESH).count()
}

fn script() -> Script {
    Script::new()
        .always(
            Route::GetNode,
            Answer::problem(ErrorCode::RateLimited).retry_after(1),
        )
        .always(
            Route::ListChildren,
            Answer::problem(ErrorCode::UpgradeRequired)
                .extra("detail", json!("Please update"))
                .extra("update_url", json!("https://updates.example/get")),
        )
        .always(
            Route::GetRevision,
            Answer::problem_with("weird_code", 418, "Teapot"),
        )
        .always(
            Route::ChangesHead,
            Answer::json(200, &json!({})).fault(Fault::DropConnection),
        )
        .always(
            Route::CreateNode,
            Answer::problem(ErrorCode::AccountNotActive),
        )
}

fn request(method: Method, target: &str, attempts: u32) -> Request {
    Request {
        method,
        target: target.to_owned(),
        headers: Vec::new(),
        body: bytes::Bytes::from_static(b"{}"),
        max_attempts: attempts,
    }
}

#[tokio::test]
async fn no_token_reaches_a_url_an_error_text_or_a_log_line() {
    let (logs, _guard) = capture_logs();
    let fixture = Fixture::start(script()).await;
    let clock = clock();
    let credentials = SentinelCredentials::new();
    let path = Arc::new(RequestPath::new(
        FixtureTransport::new(&fixture, clock.clone()),
        credentials.clone(),
        clock.clone(),
        RetryPolicy {
            base: std::time::Duration::from_millis(500),
            max: std::time::Duration::from_secs(30),
            max_attempts_per_block: 8,
        },
    ));
    let calls = [
        (Method::Get, "/api/v1/nodes/n1", 2),
        (Method::Get, "/api/v1/nodes/n1/children", 1),
        (Method::Get, "/api/v1/revisions/r1", 1),
        (Method::Get, "/api/v1/changes/head", 2),
        (Method::Post, "/api/v1/nodes", 1),
    ];

    let mut outcomes = Vec::new();
    for (method, target, attempts) in calls {
        let path = path.clone();
        let task =
            tokio::spawn(async move { path.send("test", request(method, target, attempts)).await });
        outcomes.push(drive(&clock, task).await);
    }
    path.cancel();
    outcomes.push(
        path.send("test", request(Method::Get, "/api/v1/nodes/root", 1))
            .await,
    );

    let errors: Vec<SyncError> = outcomes
        .into_iter()
        .map(|outcome| match outcome {
            Err(Mapped::Error(error)) => error,
            other => panic!("expected a typed error, got {other:?}"),
        })
        .collect();
    assert!(matches!(errors[0], SyncError::RateLimited { .. }));
    assert!(matches!(errors[1], SyncError::UpgradeRequired { .. }));
    assert!(matches!(errors[2], SyncError::Unexpected { .. }));
    assert!(matches!(errors[3], SyncError::Transport(_)));
    assert_eq!(errors[4], SyncError::AccountNotActive);
    assert_eq!(errors[5], SyncError::Cancelled);

    let error_texts: String = errors.iter().map(|e| format!("{e} {e:?}\n")).collect();
    let requests = fixture.requests();
    let urls: String = requests.iter().map(|r| format!("{}\n", r.target)).collect();
    let everything_sent: String = requests
        .iter()
        .map(|r| format!("{:?} {}", r.headers, String::from_utf8_lossy(&r.body)))
        .collect();
    let log_text = logs.text();
    assert_eq!(sentinels_in(&error_texts), 0, "{error_texts}");
    assert_eq!(sentinels_in(&urls), 0, "{urls}");
    assert_eq!(sentinels_in(&log_text), 0, "{log_text}");
    assert_eq!(everything_sent.matches(REFRESH).count(), 0);
    assert!(
        requests
            .iter()
            .all(|r| r.header("authorization") == Some("Bearer ACC-SENTINEL"))
    );
    assert!(log_text.contains("retrying request"), "{log_text}");
    assert!(log_text.contains("unexpected server answer"), "{log_text}");
    for field in ["op=", "attempt=", "status="] {
        assert!(log_text.contains(field), "no {field} in {log_text}");
    }
}

#[test]
fn credentials_print_as_stars() {
    let credential = Credential {
        access: Secret::new(ACCESS.to_owned()),
        refresh: Some(Secret::new(REFRESH.to_owned())),
    };

    let printed = [
        format!("{credential}"),
        format!("{credential:?}"),
        format!("{:#?}", credential.clone()),
        format!("{:?}", credential.access),
        format!("{}", credential.access),
    ];

    assert!(printed.iter().all(|text| text == "***"), "{printed:?}");
    let device_token = Credential {
        access: Secret::new(ACCESS.to_owned()),
        refresh: None,
    };
    assert_eq!(format!("{device_token:?}"), "***");
}

#[test]
fn a_wire_request_prints_without_its_token_or_body() {
    let wire = sync_core::WireRequest {
        method: Method::Post,
        target: "/api/v1/auth/refresh-tokens".to_owned(),
        headers: Vec::new(),
        bearer: Some(Secret::new(ACCESS.to_owned())),
        body: bytes::Bytes::from(REFRESH.as_bytes().to_vec()),
    };

    let printed = format!("{wire:?}");

    assert_eq!(sentinels_in(&printed), 0, "{printed}");
}
