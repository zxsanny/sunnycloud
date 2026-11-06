use std::fmt;
use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;
use sc_config::Secret;
use sc_error::TransportKind;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

/// One HTTP request as the transport sees it. The access token is separate from `headers` and secret:
/// the transport writes it as `Authorization: Bearer ...` and nowhere else.
#[derive(Clone)]
pub struct WireRequest {
    pub method: Method,
    /// Path and query, always starting with `/api/v1`; the transport adds scheme and host from `sync.server_url`.
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub bearer: Option<Secret<String>>,
    pub body: Bytes,
}

/// Prints the shape only: a body can carry a token (refresh, code exchange).
impl fmt::Debug for WireRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WireRequest")
            .field("method", &self.method)
            .field("target", &self.target)
            .field("bearer", &self.bearer)
            .field("body_len", &self.body.len())
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
}

impl WireResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// The HTTP executor behind the request path: one call, one request, no redirects to another host, no retry.
/// A response of any status is `Ok`; `Err` means no complete response arrived.
pub trait Transport: Send + Sync {
    fn send(&self, request: WireRequest) -> BoxFuture<'_, Result<WireResponse, TransportKind>>;
}
