use axum::routing::MethodRouter;
use sc_error::ErrorCode;
use serde_json::{Value, json};

use super::RouteContext;
use super::classes::{AuthClass, BodyClass, HttpMethod, ListenerKind, Timing};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamLocation {
    Path,
    Query,
    Header,
    Body,
}

impl ParamLocation {
    pub const fn key(self) -> &'static str {
        match self {
            ParamLocation::Path => "path",
            ParamLocation::Query => "query",
            ParamLocation::Header => "header",
            ParamLocation::Body => "body",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamType {
    Uuid,
    String,
    Integer,
}

impl ParamType {
    pub(crate) fn schema(self) -> Value {
        match self {
            ParamType::Uuid => json!({"type": "string", "format": "uuid"}),
            ParamType::String => json!({"type": "string"}),
            ParamType::Integer => json!({"type": "integer", "minimum": 0}),
        }
    }
}

/// One parameter of an operation. `id` marks an identifier of a resource: it is listed in `x-id-params`,
/// which the two-account matrix reads.
#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub name: &'static str,
    pub location: ParamLocation,
    pub ty: ParamType,
    pub id: bool,
    pub required: bool,
}

impl Param {
    /// A path variable that is not an identifier (a selector such as an index or a kind).
    pub const fn path(name: &'static str, ty: ParamType) -> Param {
        Param {
            name,
            location: ParamLocation::Path,
            ty,
            id: false,
            required: true,
        }
    }

    /// An identifier in `location`; a path identifier is always required.
    pub const fn id(location: ParamLocation, name: &'static str, ty: ParamType) -> Param {
        Param {
            name,
            location,
            ty,
            id: true,
            required: matches!(location, ParamLocation::Path),
        }
    }

    pub const fn query(name: &'static str, ty: ParamType) -> Param {
        Param {
            name,
            location: ParamLocation::Query,
            ty,
            id: false,
            required: false,
        }
    }

    pub const fn required(mut self) -> Param {
        self.required = true;
        self
    }
}

/// A named JSON schema placed under `components.schemas` and referenced from the operation.
#[derive(Clone, Copy)]
pub struct SchemaRef {
    pub name: &'static str,
    pub build: fn() -> Value,
}

impl std::fmt::Debug for SchemaRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SchemaRef").field(&self.name).finish()
    }
}

/// Everything declared about one operation. The router, the OpenAPI document and the security matrices are
/// all generated from it.
#[derive(Clone, Copy, Debug)]
pub struct RouteSpec {
    pub method: HttpMethod,
    pub path: &'static str,
    /// The port method the handler calls, as `Port::method`.
    pub owner: &'static str,
    pub auth: AuthClass,
    pub body: BodyClass,
    pub timing: Timing,
    pub state_changing: bool,
    pub listener: ListenerKind,
    pub params: &'static [Param],
    pub success: u16,
    /// Codes this operation answers with beyond those the auth and body classes imply.
    pub errors: &'static [ErrorCode],
    pub summary: &'static str,
    pub request: Option<SchemaRef>,
    pub response: Option<SchemaRef>,
}

impl RouteSpec {
    /// A public-listener operation answering 200. The state-changing flag starts from the method; a
    /// POST that only reads uses [`RouteSpec::read_only`].
    pub const fn new(
        method: HttpMethod,
        path: &'static str,
        owner: &'static str,
        auth: AuthClass,
        body: BodyClass,
    ) -> RouteSpec {
        RouteSpec {
            method,
            path,
            owner,
            auth,
            body,
            timing: Timing::Request,
            state_changing: method.mutates_by_default(),
            listener: ListenerKind::Public,
            params: &[],
            success: 200,
            errors: &[],
            summary: "",
            request: None,
            response: None,
        }
    }

    pub const fn summary(mut self, summary: &'static str) -> RouteSpec {
        self.summary = summary;
        self
    }

    pub const fn params(mut self, params: &'static [Param]) -> RouteSpec {
        self.params = params;
        self
    }

    pub const fn errors(mut self, errors: &'static [ErrorCode]) -> RouteSpec {
        self.errors = errors;
        self
    }

    pub const fn success(mut self, status: u16) -> RouteSpec {
        self.success = status;
        self
    }

    /// A held read: its time limit is the `wait` query parameter plus a margin, not `request_timeout`.
    pub const fn long_poll(mut self) -> RouteSpec {
        self.timing = Timing::LongPoll;
        self
    }

    /// A content stream: no total time cap.
    pub const fn download(mut self) -> RouteSpec {
        self.timing = Timing::Download;
        self
    }

    pub const fn internal(mut self) -> RouteSpec {
        self.listener = ListenerKind::Internal;
        self
    }

    pub const fn read_only(mut self) -> RouteSpec {
        self.state_changing = false;
        self
    }

    /// For the GET that changes state by protocol (the provider callback); the exemption list names it.
    pub const fn mutating(mut self) -> RouteSpec {
        self.state_changing = true;
        self
    }

    pub const fn request(mut self, schema: SchemaRef) -> RouteSpec {
        self.request = Some(schema);
        self
    }

    pub const fn response(mut self, schema: SchemaRef) -> RouteSpec {
        self.response = Some(schema);
        self
    }
}

/// How an operation is attached to a router: given the context and the method filter its spec declares,
/// it returns the handler.
pub type Mount = fn(&RouteContext, axum::routing::MethodFilter) -> MethodRouter;

/// A registry entry: the declaration and the handler. No route reaches a router except as one of these.
#[derive(Clone, Copy)]
pub struct RouteDef {
    pub spec: RouteSpec,
    pub mount: Mount,
}

impl RouteDef {
    pub const fn new(spec: RouteSpec, mount: Mount) -> RouteDef {
        RouteDef { spec, mount }
    }
}
