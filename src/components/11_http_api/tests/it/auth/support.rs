//! Doubles for the two authentication ports (01 and 08 belong to other components) and a surface with one
//! route per auth class whose handler answers with the caller it was given.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use axum::Json;
use axum::extract::Extension;
use axum::routing::{MethodFilter, MethodRouter, on};
use contracts::{
    Authenticator, AuthenticatorError, Credential, Principal, PrincipalKind, Role, Scope,
    WorkerAuthError, WorkerAuthenticator, WorkerIdentity,
};
use serde_json::{Value, json};
use sunnycloud_http_api::registry::{
    AuthClass, BodyClass, HttpMethod, RouteContext, RouteDef, RouteSpec, Routers, document, mount,
};
use sunnycloud_http_api::{ApiServices, Authenticated};
use uuid::Uuid;

use crate::harness::router_server::{RouterServer, route_context_with};
use crate::harness::{DEVICE_PREFIX, Script, new_metrics, stub_services};

pub const MEMBER_TOKEN: &str = "member-session";
pub const ADMIN_TOKEN: &str = "admin-session";
pub const DEVICE_TOKEN: &str = "dev-laptop";
pub const FAULT_TOKEN: &str = "fault-token-9f3a";
pub const WORKER_SECRET: &str = "worker-secret";

pub const MEMBER_ACCOUNT: Uuid = Uuid::from_u128(0xA1);
pub const ADMIN_ACCOUNT: Uuid = Uuid::from_u128(0xA2);
pub const DEVICE_ACCOUNT: Uuid = Uuid::from_u128(0xA3);
pub const MEMBER_SESSION: Uuid = Uuid::from_u128(0xB1);
pub const ADMIN_SESSION: Uuid = Uuid::from_u128(0xB2);
pub const DEVICE_ID: Uuid = Uuid::from_u128(0xB3);

/// How one credential reached 01.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presented {
    pub kind: &'static str,
    pub token: String,
}

/// 01's `Authenticator` for the tests: three live credentials in the tables they belong to, everything else
/// unknown, and 01's scope table for `require`.
#[derive(Default)]
pub struct ScriptedAuthenticator {
    presented: Mutex<Vec<Presented>>,
    required: Mutex<usize>,
}

impl ScriptedAuthenticator {
    pub fn calls(&self) -> Vec<Presented> {
        self.presented
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn required(&self) -> usize {
        *self.required.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn live() -> HashMap<&'static str, (&'static str, Principal)> {
        let principal = |account, kind, id, role| Principal {
            account_id: account,
            kind,
            session_or_device_id: id,
            role,
            label: "test".to_owned(),
        };
        HashMap::from([
            (
                MEMBER_TOKEN,
                (
                    "session",
                    principal(
                        MEMBER_ACCOUNT,
                        PrincipalKind::Session,
                        MEMBER_SESSION,
                        Role::User,
                    ),
                ),
            ),
            (
                ADMIN_TOKEN,
                (
                    "session",
                    principal(
                        ADMIN_ACCOUNT,
                        PrincipalKind::Session,
                        ADMIN_SESSION,
                        Role::Admin,
                    ),
                ),
            ),
            (
                DEVICE_TOKEN,
                (
                    "device",
                    principal(DEVICE_ACCOUNT, PrincipalKind::Device, DEVICE_ID, Role::User),
                ),
            ),
        ])
    }
}

#[contracts::async_trait]
impl Authenticator for ScriptedAuthenticator {
    async fn authenticate(&self, credential: Credential) -> Result<Principal, AuthenticatorError> {
        let (kind, token) = match &credential {
            Credential::Session(token) => ("session", token.expose()),
            Credential::Device(token) => ("device", token.expose()),
        };
        self.presented
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Presented {
                kind,
                token: token.to_owned(),
            });
        if token == FAULT_TOKEN {
            return Err(AuthenticatorError::Unexpected(Box::new(
                std::io::Error::other("database unreachable at /var/lib/sunnycloud"),
            )));
        }
        match Self::live().remove(token) {
            Some((table, principal)) if table == kind => Ok(principal),
            _ => Err(AuthenticatorError::Unauthenticated),
        }
    }

    fn require(&self, principal: &Principal, scope: Scope) -> Result<(), AuthenticatorError> {
        *self.required.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        let allowed = match scope {
            Scope::Public | Scope::SessionOrDevice => true,
            Scope::SessionOnly => principal.kind == PrincipalKind::Session,
            Scope::Admin => {
                principal.kind == PrincipalKind::Session && principal.role == Role::Admin
            }
        };
        if allowed {
            Ok(())
        } else {
            Err(AuthenticatorError::Forbidden)
        }
    }
}

/// 08's `WorkerAuthenticator`: one secret, one `Forbidden` for everything else.
#[derive(Default)]
pub struct ScriptedWorkerAuth {
    presented: Mutex<Vec<String>>,
}

impl ScriptedWorkerAuth {
    pub fn calls(&self) -> Vec<String> {
        self.presented
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[contracts::async_trait]
impl WorkerAuthenticator for ScriptedWorkerAuth {
    async fn authenticate(&self, bearer: &str) -> Result<WorkerIdentity, WorkerAuthError> {
        self.presented
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(bearer.to_owned());
        if bearer == WORKER_SECRET {
            Ok(WorkerIdentity)
        } else {
            Err(WorkerAuthError::Forbidden)
        }
    }
}

/// The caller a handler was given, as JSON.
async fn describe(
    authenticated: Option<Extension<Authenticated>>,
    worker: Option<Extension<WorkerIdentity>>,
) -> Json<Value> {
    if let Some(Extension(caller)) = authenticated {
        return Json(json!({
            "account_id": caller.principal.account_id,
            "kind": format!("{:?}", caller.principal.kind),
            "session_or_device_id": caller.principal.session_or_device_id,
            "retained_ok": caller.retained_ok,
            "source": format!("{:?}", caller.source),
        }));
    }
    if worker.is_some() {
        return Json(json!({"worker": true}));
    }
    Json(json!({"anonymous": true}))
}

fn describe_caller(_context: &RouteContext, filter: MethodFilter) -> MethodRouter {
    on(filter, describe)
}

pub fn route(method: HttpMethod, path: &'static str, auth: AuthClass) -> RouteDef {
    let spec = RouteSpec::new(method, path, "Test::route", auth, BodyClass::None)
        .summary("A test operation");
    let spec = if auth == AuthClass::Worker {
        spec.internal()
    } else {
        spec
    };
    RouteDef::new(spec, describe_caller)
}

/// One GET route per auth class.
pub const PROBES: [(AuthClass, &str); 8] = [
    (AuthClass::Public, "/api/v1/probe/public"),
    (AuthClass::Session, "/api/v1/probe/session"),
    (
        AuthClass::SessionOrDevice,
        "/api/v1/probe/session-or-device",
    ),
    (AuthClass::DeviceOnly, "/api/v1/probe/device-only"),
    (AuthClass::SessionOnly, "/api/v1/probe/session-only"),
    (AuthClass::Admin, "/api/v1/probe/admin"),
    (AuthClass::ShareGrant, "/api/v1/probe/share-grant"),
    (AuthClass::Worker, "/api/v1/probe/worker"),
];

pub fn path_of(class: AuthClass) -> &'static str {
    PROBES
        .iter()
        .find(|(candidate, _)| *candidate == class)
        .map(|(_, path)| *path)
        .expect("every class has a probe route")
}

pub fn class_routes() -> Vec<RouteDef> {
    PROBES
        .into_iter()
        .map(|(class, path)| route(HttpMethod::Get, path, class))
        .collect()
}

/// The routers of `defs` over the two authentication doubles, and everything else a stub.
pub struct Surface {
    pub routers: Routers,
    pub authenticator: Arc<ScriptedAuthenticator>,
    pub worker_auth: Arc<ScriptedWorkerAuth>,
}

pub fn surface(defs: &[RouteDef]) -> Surface {
    let authenticator = Arc::new(ScriptedAuthenticator::default());
    let worker_auth = Arc::new(ScriptedWorkerAuth::default());
    let services = ApiServices {
        authenticator: authenticator.clone(),
        worker_auth: worker_auth.clone(),
        ..stub_services(&Script::default(), new_metrics())
    };
    let context = RouteContext {
        services: Arc::new(services),
        ..route_context_with(document(defs), &Script::default())
    };
    Surface {
        routers: mount(defs, &context, None),
        authenticator,
        worker_auth,
    }
}

pub struct Running {
    pub public: RouterServer,
    pub internal: RouterServer,
    pub authenticator: Arc<ScriptedAuthenticator>,
    pub worker_auth: Arc<ScriptedWorkerAuth>,
}

impl Running {
    pub async fn start(defs: &[RouteDef]) -> Running {
        let Surface {
            routers,
            authenticator,
            worker_auth,
        } = surface(defs);
        Running {
            public: RouterServer::start(routers.public).await,
            internal: RouterServer::start(routers.internal).await,
            authenticator,
            worker_auth,
        }
    }

    pub async fn stop(self) {
        self.public.stop().await;
        self.internal.stop().await;
    }
}

/// A device-marked token that 01 does not know.
pub fn unknown_device_token() -> String {
    format!("{DEVICE_PREFIX}unknown")
}

/// The body of an answer without the per-request id, to compare answers.
pub fn without_request_id(mut body: Value) -> Value {
    body.as_object_mut()
        .expect("problem body is an object")
        .remove("request_id")
        .expect("problem body has request_id");
    body
}

/// `method path` over a fresh connection, with `token` as the bearer when given.
pub async fn call(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
) -> crate::harness::router_server::Answer {
    let value = token.map(|token| format!("Bearer {token}"));
    let headers: Vec<(&str, &str)> = value
        .iter()
        .map(|value| ("authorization", value.as_str()))
        .collect();
    crate::harness::router_server::request_with(addr, method, path, &headers).await
}
