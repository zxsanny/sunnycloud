//! The HTTP API library (11): the hand-built accept loop, the listeners and the surface the composition root
//! (12) runs.
//!
//! Contract: `_docs/02_document/contracts/11_http_api/http_server.md`. 11 builds no service and binds no
//! configuration: it holds the ports of [`ApiServices`] and the typed [`HttpConfig`] it is given.
//!
//! [`AcceptLoop`] is the one loop every listener runs: one task per connection, a global connection cap,
//! HTTP/1.1 and h2c prior knowledge on one port, and the configured HTTP/2 and header-timeout settings.

mod accept;
mod config;
mod error;
mod gate;
mod metrics;
mod server;
mod services;

pub use accept::AcceptLoop;
pub use config::{HttpConfig, ServerInfo};
pub use error::{HttpServerError, Listener};
pub use server::{BoundServer, HttpServer, OpenApiDocument};
pub use services::ApiServices;
