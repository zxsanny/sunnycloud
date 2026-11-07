//! The route registry: the one list of operations from which the routers, the OpenAPI document and the
//! security matrices are generated (`api_v1_surface.md`). A route reaches a router only as a [`RouteDef`].

mod classes;
mod document;
mod mount;
mod naming;
mod spec;
mod system;
mod validate;

use std::sync::Arc;

pub use classes::{AuthClass, BodyClass, HttpMethod, ListenerKind};
pub use document::{OpenApiDocument, document};
pub use mount::{Routers, mount};
pub use naming::{NamingViolation, lint_path};
pub use spec::{Mount, Param, ParamLocation, ParamType, RouteDef, RouteSpec, SchemaRef};
pub use validate::{RegistryError, validate};

use crate::auth::CredentialFormat;
use crate::config::ServerInfo;
use crate::services::ApiServices;

/// What a handler is built from.
pub struct RouteContext {
    pub services: Arc<ApiServices>,
    pub openapi: OpenApiDocument,
    pub server_info: ServerInfo,
    pub credentials: CredentialFormat,
}

/// Every operation the server offers. Each group of E-21 adds its own list here.
pub fn production_routes() -> Vec<RouteDef> {
    system::routes()
}
