use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Json;
use axum::routing::{MethodFilter, MethodRouter, on};
use serde_json::{Value, json};
use sunnycloud_http_api::registry::{
    AuthClass, BodyClass, HttpMethod, RouteContext, RouteDef, RouteSpec,
};

use crate::auth::support::{ADMIN_TOKEN, MEMBER_TOKEN};
use crate::harness::SESSION_COOKIE;
use crate::harness::router_server::{Answer, request_with};

pub const MUTATIONS: [(HttpMethod, &str); 4] = [
    (HttpMethod::Post, "POST"),
    (HttpMethod::Put, "PUT"),
    (HttpMethod::Patch, "PATCH"),
    (HttpMethod::Delete, "DELETE"),
];

/// One path per user auth class and the token of a caller that class accepts.
pub const GROUPS: [(AuthClass, &str, &str); 4] = [
    (
        AuthClass::Session,
        "/api/v1/probes/csrf-session",
        MEMBER_TOKEN,
    ),
    (
        AuthClass::SessionOrDevice,
        "/api/v1/probes/csrf-session-or-device",
        MEMBER_TOKEN,
    ),
    (
        AuthClass::SessionOnly,
        "/api/v1/probes/csrf-session-only",
        MEMBER_TOKEN,
    ),
    (AuthClass::Admin, "/api/v1/probes/csrf-admin", ADMIN_TOKEN),
];

pub const PUBLIC_PATH: &str = "/api/v1/probes/csrf-public";

/// Counts the runs of state-changing handlers that only the tests of `reads` touch.
pub static STATE_CHANGES: AtomicUsize = AtomicUsize::new(0);

async fn handled() -> Json<Value> {
    Json(json!({"handled": true}))
}

async fn counted() -> Json<Value> {
    STATE_CHANGES.fetch_add(1, Ordering::SeqCst);
    Json(json!({"handled": true}))
}

fn handler(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, handled)
}

fn counting_handler(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, counted)
}

fn body_of(method: HttpMethod) -> BodyClass {
    if method == HttpMethod::Delete {
        BodyClass::None
    } else {
        BodyClass::Small
    }
}

fn mutation(method: HttpMethod, path: &'static str, auth: AuthClass, count: bool) -> RouteDef {
    let spec = RouteSpec::new(method, path, "Test::mutate", auth, body_of(method))
        .summary("A test mutation");
    RouteDef::new(spec, if count { counting_handler } else { handler })
}

/// Every mutation method on the path of every user auth class, and on a public path.
pub fn mutation_routes() -> Vec<RouteDef> {
    let mut defs = Vec::new();
    for (method, _) in MUTATIONS {
        for (class, path, _) in GROUPS {
            defs.push(mutation(method, path, class, false));
        }
        defs.push(mutation(method, PUBLIC_PATH, AuthClass::Public, false));
    }
    defs
}

/// A read of every user class and a counted mutation on the same path.
pub fn read_routes() -> Vec<RouteDef> {
    let mut defs = Vec::new();
    for (class, path, _) in GROUPS {
        for method in [HttpMethod::Get, HttpMethod::Head] {
            let spec = RouteSpec::new(method, path, "Test::read", class, BodyClass::None)
                .summary("A test read");
            defs.push(RouteDef::new(spec, handler));
        }
        defs.push(mutation(HttpMethod::Post, path, class, true));
    }
    defs
}

pub fn cookie_of(token: &str) -> String {
    format!("{SESSION_COOKIE}={token}")
}

/// `method path` with the session cookie of `token` and `extra` headers.
pub async fn with_cookie(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    token: &str,
    extra: &[(&str, &str)],
) -> Answer {
    let cookie = cookie_of(token);
    let mut headers = vec![("cookie", cookie.as_str())];
    headers.extend_from_slice(extra);
    request_with(addr, method, path, &headers).await
}

pub fn handler_ran(answer: &Answer) -> bool {
    answer.status == 200 && answer.json()["handled"] == true
}
