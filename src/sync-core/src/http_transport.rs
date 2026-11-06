//! The production [`Transport`]: one reqwest HTTP/2 client over rustls. It never follows a redirect, never
//! retries, and sends every request to the origin of `sync.server_url` only.

use std::error::Error;
use std::time::Duration;

use bytes::BytesMut;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Url, redirect};
use rustls::pki_types::CertificateDer;
use sc_error::TransportKind;
use sync_core_proto::ServerUrl;
use thiserror::Error;

use crate::config::SyncConfig;
use crate::request::{BoxFuture, Method, Transport, WireRequest, WireResponse};
use crate::tls;

pub(crate) const SERVER_INFO_PATH: &str = "/api/v1/server-info";
const CLIENT_HEADER: HeaderName = HeaderName::from_static("x-sunnycloud-client");
const DEFAULT_ACCEPT: &str = "application/json, application/problem+json";
/// Largest response body held in memory: a block is 2 MiB, every JSON answer is far smaller (13 §7).
pub(crate) const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
/// HTTP/2 flow-control windows cannot exceed 2^31 - 1.
const MAX_WINDOW: u64 = (1 << 31) - 1;

#[derive(Debug, Error)]
pub(crate) enum SetupError {
    #[error("tls setup failed: {0}")]
    Tls(#[from] rustls::Error),
    #[error("http client setup failed: {0}")]
    Client(#[from] reqwest::Error),
    #[error("invalid client header value")]
    ClientHeader,
}

pub(crate) struct HttpTransport {
    client: Client,
    base: Url,
    client_header: HeaderValue,
}

impl HttpTransport {
    /// `client_header` is `<family>/<semver>`. Building opens no socket.
    pub(crate) fn new(
        config: &SyncConfig,
        base: &ServerUrl,
        client_header: &str,
        extra_roots: Vec<CertificateDer<'static>>,
    ) -> Result<Self, SetupError> {
        let tls = tls::client_config(extra_roots)?;
        let client = Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(!cfg!(feature = "allow-plain-http"))
            .http2_prior_knowledge()
            .http2_initial_stream_window_size(window(config.h2_stream_window))
            .http2_initial_connection_window_size(window(config.h2_conn_window))
            .http2_adaptive_window(false)
            .http2_keep_alive_interval(config.keepalive_interval)
            .http2_keep_alive_while_idle(true)
            .pool_idle_timeout(None::<Duration>)
            .connect_timeout(config.connect_timeout)
            .read_timeout(config.request_idle_timeout)
            .redirect(redirect::Policy::none())
            .no_proxy()
            .build()?;
        let base = Url::parse(base.as_str()).map_err(|_| SetupError::ClientHeader)?;
        let client_header =
            HeaderValue::from_str(client_header).map_err(|_| SetupError::ClientHeader)?;
        Ok(Self {
            client,
            base,
            client_header,
        })
    }

    fn request(&self, wire: WireRequest) -> Result<reqwest::RequestBuilder, TransportKind> {
        let url = self.url(&wire.target)?;
        let mut headers = HeaderMap::new();
        for (name, value) in &wire.headers {
            let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid("header"))?;
            let value = HeaderValue::from_str(value).map_err(|_| invalid("header"))?;
            headers.append(name, value);
        }
        if !headers.contains_key(ACCEPT) {
            headers.insert(ACCEPT, HeaderValue::from_static(DEFAULT_ACCEPT));
        }
        // The caller's headers cannot replace these two.
        headers.remove(AUTHORIZATION);
        headers.remove(CLIENT_HEADER);
        if url.path() != self.base_path(SERVER_INFO_PATH) {
            headers.insert(CLIENT_HEADER, self.client_header.clone());
        }
        if let Some(bearer) = wire.bearer.as_ref().filter(|b| !b.expose().is_empty()) {
            let mut value = HeaderValue::from_str(&format!("Bearer {}", bearer.expose()))
                .map_err(|_| invalid("credential"))?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
        let mut builder = self
            .client
            .request(reqwest_method(wire.method), url)
            .headers(headers);
        if !wire.body.is_empty() {
            builder = builder.body(wire.body);
        }
        Ok(builder)
    }

    /// The target joined to the server URL; it must stay on that origin.
    fn url(&self, target: &str) -> Result<Url, TransportKind> {
        if !target.starts_with("/api/v1") {
            return Err(invalid("target"));
        }
        let url = Url::parse(&format!(
            "{}{target}",
            self.base.as_str().trim_end_matches('/')
        ))
        .map_err(|_| invalid("target"))?;
        if url.origin() != self.base.origin() {
            return Err(invalid("target"));
        }
        Ok(url)
    }

    fn base_path(&self, route: &str) -> String {
        format!("{}{route}", self.base.path().trim_end_matches('/'))
    }

    async fn execute(&self, wire: WireRequest) -> Result<WireResponse, TransportKind> {
        let mut response = self.request(wire)?.send().await.map_err(classify)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BODY_BYTES as u64)
        {
            return Err(too_large());
        }
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        let mut body = BytesMut::new();
        while let Some(chunk) = response.chunk().await.map_err(classify)? {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(WireResponse {
            status,
            headers,
            body: body.freeze(),
        })
    }
}

impl Transport for HttpTransport {
    fn send(&self, request: WireRequest) -> BoxFuture<'_, Result<WireResponse, TransportKind>> {
        Box::pin(self.execute(request))
    }
}

fn window(bytes: u64) -> u32 {
    u32::try_from(bytes.min(MAX_WINDOW)).unwrap_or(u32::MAX)
}

fn reqwest_method(method: Method) -> reqwest::Method {
    match method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
    }
}

fn invalid(what: &'static str) -> TransportKind {
    tracing::error!(what, "request refused before sending");
    TransportKind::Other
}

fn too_large() -> TransportKind {
    tracing::error!(limit = MAX_BODY_BYTES, "response body over the limit");
    TransportKind::Other
}

/// Logs the cause chain without the URL, which can carry node ids.
fn classify(error: reqwest::Error) -> TransportKind {
    let kind = if error.is_timeout() {
        TransportKind::Timeout
    } else if error.is_connect() {
        TransportKind::Connect
    } else {
        TransportKind::Other
    };
    let error = error.without_url();
    let mut chain = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        chain.push_str(": ");
        chain.push_str(&cause.to_string());
        source = cause.source();
    }
    tracing::warn!(?kind, error = %chain, "request failed");
    kind
}
