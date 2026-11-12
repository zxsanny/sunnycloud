//! HTTP access to the release source: GET only, redirects only as [`RedirectRule`] allows, no proxy, bounded
//! bodies. TLS is the transport, not the trust anchor: every byte is checked against the signed manifest.

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::CONTENT_LENGTH;
use reqwest::{Client, StatusCode, redirect};
use rustls::ClientConfig;
use rustls::crypto::ring;
use rustls_platform_verifier::BuilderVerifierExt;
use sha2::{Digest, Sha256};

use super::redirect::RedirectRule;
use super::verify::Rejection;

/// Largest manifest or signature file read; both are a few hundred bytes.
const MAX_SMALL_FILE: usize = 64 * 1024;

pub(super) struct ReleaseClient {
    client: Client,
}

/// A downloaded payload whose size and digest matched the signed entry.
pub(super) struct Payload(pub(super) Vec<u8>);

impl ReleaseClient {
    pub(super) fn new(
        connect_timeout: Duration,
        request_timeout: Duration,
        rule: RedirectRule,
    ) -> Result<Self, String> {
        let provider = Arc::new(ring::default_provider());
        let tls = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|error| error.to_string())?
            .with_platform_verifier()
            .map_err(|error| error.to_string())?
            .with_no_client_auth();
        let client = Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(!cfg!(any(test, feature = "test-update-trust")))
            .redirect(redirect::Policy::custom(move |attempt| {
                match rule.check(attempt.url(), attempt.previous().len()) {
                    Ok(()) => attempt.follow(),
                    Err(refused) => attempt.error(refused),
                }
            }))
            .no_proxy()
            .connect_timeout(connect_timeout)
            .timeout(request_timeout)
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { client })
    }

    /// The file's bytes, or `None` on 404 (an absent release file).
    pub(super) async fn get_small(&self, url: &str) -> Result<Option<Vec<u8>>, Rejection> {
        let mut response = self.client.get(url).send().await.map_err(source)?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Ok(None),
            status => return Err(unexpected(status)),
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(source)? {
            if body.len() + chunk.len() > MAX_SMALL_FILE {
                return Err(Rejection::Source("the file is too large".to_owned()));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Some(body))
    }

    /// Downloads the payload, refusing it on a size mismatch (checked first, so a truncated file is a
    /// size failure) and then on a digest mismatch. Never reads more than `size` bytes.
    pub(super) async fn download(
        &self,
        url: &str,
        size: u64,
        sha256: &[u8; 32],
    ) -> Result<Payload, Rejection> {
        let mut response = self.client.get(url).send().await.map_err(source)?;
        if response.status() != StatusCode::OK {
            return Err(unexpected(response.status()));
        }
        let declared = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|text| text.parse::<u64>().ok());
        if declared.is_some_and(|declared| declared != size) {
            return Err(Rejection::Size);
        }
        let capacity = usize::try_from(size).map_err(|_| Rejection::Size)?;
        let mut body = Vec::with_capacity(capacity.min(64 << 20));
        while let Some(chunk) = response.chunk().await.map_err(source)? {
            if body.len() + chunk.len() > capacity {
                return Err(Rejection::Size);
            }
            body.extend_from_slice(&chunk);
        }
        if body.len() != capacity {
            return Err(Rejection::Size);
        }
        if Sha256::digest(&body).as_slice() != sha256.as_slice() {
            return Err(Rejection::Digest);
        }
        Ok(Payload(body))
    }
}

/// The error with its causes, so a refused redirect names why.
fn source(error: reqwest::Error) -> Rejection {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(inner) = cause {
        text.push_str(&format!(": {inner}"));
        cause = inner.source();
    }
    Rejection::Source(text)
}

fn unexpected(status: StatusCode) -> Rejection {
    Rejection::Source(format!("unexpected HTTP status {}", status.as_u16()))
}
