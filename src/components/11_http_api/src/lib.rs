//! The HTTP API library (11): the hand-built accept loop, the listeners (public, internal, ops) and the surface
//! the composition root (12) runs.
//!
//! Contract: `_docs/02_document/contracts/11_http_api/http_server.md`. 11 builds no service and binds no
//! configuration: it holds the ports of [`ApiServices`] and the typed [`HttpConfig`] it is given.
//!
//! [`ApiProblem`] is the one place an error becomes an answer: status from the `sc_error` table, headers and the
//! `application/problem+json` body (`problem_details.md`); [`PortError`] converts each port's error to it.
//!
//! [`registry`] is the one list of operations: the routers, the OpenAPI document and the security matrices are
//! generated from it, and [`diff_documents`] is the additive-only gate between two documents.
//!
//! [`Middleware`] is layers 1 to 5 of the stack (panic catcher, client IP, request id, metrics and log,
//! security headers) that wrap the public and internal routers; handlers read the id with
//! `Extension<CorrelationId>` and the caller's address with `Extension<ClientIp>`.
//!
//! [`BodyLimits`] is layer 6, the innermost of a route: the body limit and the time limits of its `BodyClass` and
//! `Timing`, applied after the version gate and auth so a refused caller never has a body byte read.
//!
//! [`VersionGate`] is layer 7: it refuses a client below `min_client_version` with 426 before auth, and serves
//! the count of client versions; [`ServerInfo`] is what `GET /api/v1/server-info` returns.
//!
//! [`CsrfGuard`] is layer 9: after auth, a cookie-authenticated POST, PUT, PATCH or DELETE must prove the
//! public origin (`Sec-Fetch-Site: same-origin` or an exact `Origin`) or it is 403 `csrf_rejected`; bearers are exempt.
//!
//! [`content_response`] is the one mapping from 03's `Download` to the status and headers of every content
//! route (`content_serving_headers.md`): hostile types are attachments, Range and validators are the same
//! everywhere, and a failed stream aborts the connection or stream.
//!
//! The worker routes (`registry::worker`) are registered for the internal listener only; with feature `test-clock`
//! the `-test` image adds the clock-offset read there and a `:9091` listener for `POST /test/clock`.
//!
//! [`AcceptLoop`] is the one loop every listener runs: one task per connection, a global connection cap,
//! HTTP/1.1 and h2c prior knowledge on one port, and the configured HTTP/2 and header-timeout settings.
//! [`AcceptLoop::close_connections`] ends its connections at shutdown: a graceful close, then a drop at the end of
//! the grace (the order of the steps is on [`BoundServer::serve`]).

mod accept;
mod auth;
mod config;
mod content;
mod csrf;
mod error;
mod gate;
mod json_body;
mod limits;
mod metrics;
mod middleware;
mod openapi_diff;
mod port_error;
mod problem;
mod problem_schema;
pub mod registry;
mod server;
mod server_info;
mod services;
mod spa;
#[cfg(feature = "test-clock")]
mod test_clock;
mod version;
mod write_idle;

pub use accept::AcceptLoop;
pub use auth::{Authenticated, CredentialFormat, CredentialSource};
pub use config::{HttpConfig, WorkerOutputLimits};
pub use content::content_response;
pub use csrf::CsrfGuard;
pub use error::{HttpServerError, Listener};
pub use limits::BodyLimits;
pub use metrics::{
    ClientVersionLabels, DurationLabels, HttpMetrics, MethodLabel, RequestLabels, StatusClass,
};
pub use middleware::{ClientIp, Middleware};
pub use openapi_diff::{Breaking, diff as diff_documents};
pub use port_error::{PortError, entry_error_code};
pub use problem::ApiProblem;
pub use problem_schema::problem_schema;
pub use registry::OpenApiDocument;
pub use server::{BoundServer, HttpServer, current_document};
pub use server_info::ServerInfo;
pub use services::ApiServices;
pub use spa::SpaBundle;
pub use version::{CLIENT_HEADER, ClientFamily, VersionGate};
