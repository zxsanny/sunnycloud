use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::extract::Extension;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;
use sc_clock::Clock;
use sc_error::{BusinessError, CorrelationId, DomainError};

use super::RouteContext;
use super::classes::{HttpMethod, ListenerKind};
use super::spec::RouteDef;
use crate::auth::protect;
use crate::problem::ApiProblem;
use crate::spa::SpaBundle;

/// The two routers generated from one registry.
pub struct Routers {
    pub public: Router,
    pub internal: Router,
}

/// The only place a route is added to a router. The public router answers every other `/api` path with a
/// 404 problem and every non-`/api` path with the bundle's index (a 404 problem without a bundle); the
/// internal router answers every other path with a 404 problem.
pub fn mount(defs: &[RouteDef], context: &RouteContext, spa: Option<Arc<SpaBundle>>) -> Routers {
    let clock = Arc::clone(&context.services.clock);
    let public = router_for(defs, context, ListenerKind::Public).fallback({
        let clock = Arc::clone(&clock);
        move |method: Method, uri: Uri, request_id: Option<Extension<CorrelationId>>| {
            let spa = spa.clone();
            let clock = Arc::clone(&clock);
            async move {
                let request_id = request_id_of(request_id, clock.as_ref());
                public_fallback(&method, &uri, spa.as_deref(), request_id)
            }
        }
    });
    let internal = router_for(defs, context, ListenerKind::Internal).fallback(
        move |request_id: Option<Extension<CorrelationId>>| {
            let clock = Arc::clone(&clock);
            async move { not_found(request_id_of(request_id, clock.as_ref())) }
        },
    );
    Routers { public, internal }
}

/// The methods declared on one path.
struct PathEntry {
    handler: MethodRouter,
    get: bool,
    head: bool,
}

/// axum answers HEAD from a GET handler. A HEAD that no operation declares is refused instead, so the
/// router answers exactly the documented operations.
fn router_for(defs: &[RouteDef], context: &RouteContext, listener: ListenerKind) -> Router {
    let mut by_path: BTreeMap<&str, PathEntry> = BTreeMap::new();
    for def in defs.iter().filter(|def| def.spec.listener == listener) {
        let handler = protect(
            (def.mount)(context, def.spec.method.filter()),
            def.spec.auth,
            &context.services,
            &context.credentials,
        );
        let handler = context.version_gate.guard(handler, def.spec.auth);
        let (get, head) = (
            def.spec.method == HttpMethod::Get,
            def.spec.method == HttpMethod::Head,
        );
        let entry = match by_path.remove(def.spec.path) {
            Some(existing) => PathEntry {
                handler: existing.handler.merge(handler),
                get: existing.get || get,
                head: existing.head || head,
            },
            None => PathEntry { handler, get, head },
        };
        by_path.insert(def.spec.path, entry);
    }
    by_path
        .into_iter()
        .fold(Router::new(), |router, (path, entry)| {
            let handler = if entry.get && !entry.head {
                entry.handler.head(refuse_head)
            } else {
                entry.handler
            };
            router.route(path, handler)
        })
}

async fn refuse_head() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

fn public_fallback(
    method: &Method,
    uri: &Uri,
    spa: Option<&SpaBundle>,
    request_id: CorrelationId,
) -> Response {
    let path = uri.path();
    let is_api = path == "/api" || path.starts_with("/api/");
    let reads = matches!(*method, Method::GET | Method::HEAD);
    match spa {
        Some(bundle) if !is_api && reads => bundle.serve(method, path),
        _ => not_found(request_id),
    }
}

/// The id the request layer assigned; a router mounted without that layer takes a fresh one.
fn request_id_of(assigned: Option<Extension<CorrelationId>>, clock: &dyn Clock) -> CorrelationId {
    match assigned {
        Some(Extension(id)) => id,
        None => CorrelationId::new(sc_ids::new_id(clock)),
    }
}

fn not_found(request_id: CorrelationId) -> Response {
    ApiProblem::new(
        DomainError::Business(BusinessError::not_found()),
        request_id,
    )
    .into_response()
}
