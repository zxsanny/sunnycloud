//! The HTTP API library (11): the hand-built accept loop, the listeners (public, internal, ops) and the surface
//! the composition root (12) runs.
//!
//! Contract: `_docs/02_document/contracts/11_http_api/http_server.md`. 11 builds no service and binds no
//! configuration: it holds the ports of [`ApiServices`] and the typed [`HttpConfig`] it is given.
//!
//! [`ApiProblem`] is the one place an error becomes an answer: status from the `sc_error` table, headers and the
//! `application/problem+json` body (`problem_details.md`); [`PortError`] converts each port's error to it.
//!
//! [`AcceptLoop`] is the one loop every listener runs: one task per connection, a global connection cap,
//! HTTP/1.1 and h2c prior knowledge on one port, and the configured HTTP/2 and header-timeout settings.

mod accept;
mod config;
mod error;
mod gate;
mod metrics;
mod port_error;
mod problem;
mod problem_schema;
mod server;
mod services;

pub use accept::AcceptLoop;
pub use config::{HttpConfig, ServerInfo};
pub use error::{HttpServerError, Listener};
pub use metrics::{HttpMetrics, RequestLabels, StatusClass};
pub use port_error::{PortError, entry_error_code};
pub use problem::ApiProblem;
pub use problem_schema::problem_schema;
pub use server::{BoundServer, HttpServer, OpenApiDocument};
pub use services::ApiServices;
