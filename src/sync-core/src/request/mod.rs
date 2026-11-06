//! The request path every operation goes through (credentials, cancellation, rate limits, logs).
//! The HTTP executor behind it is a [`Transport`]; its implementation belongs to `connect` (E-23/c07).

mod jitter;
mod path;
mod transport;

pub use path::{Request, RequestPath};
pub use transport::{BoxFuture, Method, Transport, WireRequest, WireResponse};
