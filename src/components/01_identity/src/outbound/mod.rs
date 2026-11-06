//! The only way the server calls an identity provider: one reqwest client that follows no redirect, refuses
//! non-HTTPS and link-local or metadata destinations (also after DNS resolution, at connect time), reads at
//! most [`MAX_BODY_BYTES`] and goes through no proxy. The deadline is the caller's: `IdentitySignIn` bounds
//! every provider exchange by `provider_timeout`.

mod destination;
#[cfg(test)]
mod tests;

use std::error::Error;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use oauth2::{AsyncHttpClient, HttpRequest, HttpResponse};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{Client, redirect};
use rustls::ClientConfig;
use rustls::crypto::ring;
use rustls_platform_verifier::Verifier;
use thiserror::Error;

use destination::{address_refused, url_refused};

/// Largest provider response held in memory; discovery, JWKS, token and profile answers are far smaller.
const MAX_BODY_BYTES: usize = 1024 * 1024;

type LookupFuture = Pin<Box<dyn Future<Output = io::Result<Vec<SocketAddr>>> + Send>>;
/// Name to addresses; the system resolver in production, a table in the guard's own tests.
type Lookup = Arc<dyn Fn(String) -> LookupFuture + Send + Sync>;

#[derive(Debug, Error)]
pub(crate) enum SetupError {
    #[error("tls setup failed")]
    Tls(#[from] rustls::Error),
    #[error("http client setup failed")]
    Client(#[from] reqwest::Error),
}

#[derive(Debug, Error)]
pub(crate) enum OutboundError {
    /// Scheme, literal address, metadata host name, or a name that resolved only to refused addresses.
    #[error("destination refused")]
    Refused,
    #[error("redirect refused")]
    Redirect,
    #[error("response body over the limit")]
    TooLarge,
    #[error("request failed")]
    Request(#[source] reqwest::Error),
}

#[derive(Debug, Error)]
#[error("resolved only to refused addresses")]
struct ResolvedRefused;

pub(crate) struct OutboundClient {
    client: Client,
    allow_plain_http: bool,
}

impl OutboundClient {
    /// `allow_plain_http` is the development flag (`allow_plain_http_dev`). Building opens no socket.
    pub(crate) fn new(allow_plain_http: bool) -> Result<Self, SetupError> {
        let lookup: Lookup = Arc::new(|host| {
            Box::pin(
                async move { Ok(tokio::net::lookup_host((host.as_str(), 0)).await?.collect()) },
            )
        });
        Self::with_lookup(allow_plain_http, lookup)
    }

    fn with_lookup(allow_plain_http: bool, lookup: Lookup) -> Result<Self, SetupError> {
        let provider = Arc::new(ring::default_provider());
        let verifier = Verifier::new(Arc::clone(&provider))?;
        // `dangerous()` is rustls's name for installing any verifier; this one is the platform verifier.
        let mut tls = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        let client = Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(!allow_plain_http)
            .http1_only()
            .redirect(redirect::Policy::none())
            .no_proxy()
            .dns_resolver(Arc::new(GuardedResolver { lookup }))
            .build()?;
        Ok(OutboundClient {
            client,
            allow_plain_http,
        })
    }

    /// One request. A redirect answer is an error, never followed; any other status is returned for the
    /// caller to interpret.
    pub(crate) async fn send(&self, request: HttpRequest) -> Result<HttpResponse, OutboundError> {
        let request = reqwest::Request::try_from(request).map_err(OutboundError::Request)?;
        if url_refused(request.url(), self.allow_plain_http) {
            return Err(OutboundError::Refused);
        }
        let mut response = self.client.execute(request).await.map_err(classify)?;
        if response.status().is_redirection() {
            return Err(OutboundError::Redirect);
        }
        let status = response.status();
        let headers = response.headers().clone();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(classify)? {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(OutboundError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let mut answer = HttpResponse::new(body);
        *answer.status_mut() = status;
        *answer.headers_mut() = headers;
        Ok(answer)
    }
}

impl<'c> AsyncHttpClient<'c> for OutboundClient {
    type Error = OutboundError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, OutboundError>> + Send + 'c>>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(self.send(request))
    }
}

fn classify(error: reqwest::Error) -> OutboundError {
    let mut cause: Option<&(dyn Error + 'static)> = Some(&error);
    while let Some(current) = cause {
        if current.is::<ResolvedRefused>() {
            return OutboundError::Refused;
        }
        cause = current.source();
    }
    OutboundError::Request(error)
}

/// Resolves through `lookup` and hands the connector only addresses that pass [`address_refused`], so the
/// address that is dialed is the address that was checked.
struct GuardedResolver {
    lookup: Lookup,
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let lookup = Arc::clone(&self.lookup);
        Box::pin(async move {
            let resolved = lookup(name.as_str().to_owned()).await?;
            let allowed: Vec<SocketAddr> = resolved
                .into_iter()
                .filter(|address| !address_refused(address.ip()))
                .collect();
            if allowed.is_empty() {
                return Err(ResolvedRefused.into());
            }
            Ok(Box::new(allowed.into_iter()) as Addrs)
        })
    }
}
