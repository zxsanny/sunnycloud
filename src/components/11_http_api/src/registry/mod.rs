//! The route registry: the one list of operations from which the routers, the OpenAPI document and the
//! security matrices are generated (`api_v1_surface.md`). A route reaches a router only as a [`RouteDef`].

mod account;
mod admin;
mod classes;
mod devices;
mod document;
mod feed;
mod media;
mod mount;
mod naming;
mod request;
mod sessions;
mod sharing;
mod signin;
mod spec;
mod system;
mod transfer;
mod tree;
mod validate;
mod worker;

use std::sync::Arc;

pub use classes::{AuthClass, BodyClass, HttpMethod, ListenerKind, Timing};
pub use document::{OpenApiDocument, document};
pub use mount::{Routers, mount};
pub use naming::{NamingViolation, lint_path};
pub use spec::{Mount, Param, ParamLocation, ParamType, RouteDef, RouteSpec, SchemaRef};
pub use validate::{RegistryError, validate};

use crate::auth::CredentialFormat;
use crate::csrf::CsrfGuard;
use crate::limits::BodyLimits;
use crate::server_info::ServerInfo;
use crate::services::ApiServices;
use crate::version::VersionGate;

/// What a handler is built from.
pub struct RouteContext {
    pub services: Arc<ApiServices>,
    /// The configured `public_url`: the web app's origin, from which the device grant names its approval page.
    pub public_url: String,
    pub openapi: OpenApiDocument,
    pub server_info: ServerInfo,
    pub credentials: CredentialFormat,
    pub version_gate: Arc<VersionGate>,
    pub csrf: Arc<CsrfGuard>,
    pub body_limits: Arc<BodyLimits>,
}

/// Every operation the server offers. Each group of E-21 adds its own list here.
pub fn production_routes() -> Vec<RouteDef> {
    let mut routes = system::routes();
    routes.extend(signin::routes());
    routes.extend(sessions::routes());
    routes.extend(devices::routes());
    routes.extend(account::routes());
    routes.extend(admin::routes());
    routes.extend(sharing::routes());
    routes.extend(media::routes());
    routes.extend(tree::routes());
    routes.extend(transfer::routes());
    routes.extend(feed::routes());
    routes.extend(worker::routes());
    #[cfg(feature = "test-clock")]
    routes.extend(crate::test_clock::routes());
    routes
}
