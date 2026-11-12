//! The supervisor's HTTP client of the API worker listener (`:8081`, 09 section 2, `worker_job_protocol.md`).
//!
//! One method per route, one typed [`ApiError`] per failed call. Status handling follows 09 section 2:
//! 404 closes the job `obsolete` ([`ApiError::Gone`]), 409 stops quietly ([`ApiError::LeaseLost`]), 403 stops
//! claiming ([`ApiError::Forbidden`]). A status counts only on the routes that document it, so a 404 from a
//! mis-routed claim is never read as an obsolete revision. The client measures no lease or backoff time and
//! sets no request timeout: no key defines one, so callers bound a call themselves.

#[cfg(feature = "test-clock")]
mod clock;
mod error;
mod input;
mod output;
mod rollup;

use media_shared::types::{
    ClaimedJob, FailResult, FailureReport, Lane, LeaseExtended, SkipOutcome, WorkerStatus,
};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::{Client, RequestBuilder, Response, StatusCode, redirect};
use rustls::{ClientConfig, RootCertStore};
use sc_config::{Secret, WorkerSettings};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

pub use error::{ApiConfigError, ApiError};

const API_PREFIX: &str = "/api/v1/worker-jobs";
const WORKER_ID_HEADER: &str = "X-Worker-Id";
const JSON: HeaderValue = HeaderValue::from_static("application/json");

/// reqwest needs a TLS configuration when the workspace enables its rustls feature, even for a URL that never
/// uses it. This one trusts no server, so an `https` address could never connect; `new` refuses one anyway.
fn no_server_tls() -> Result<ClientConfig, ApiConfigError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| ApiConfigError::Client(error.to_string()))?
        .with_root_certificates(RootCertStore::empty())
        .with_no_client_auth();
    Ok(config)
}

/// What the client needs from the worker settings, plus the resolved `worker_id`.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub base_url: String,
    pub token: Secret<String>,
    pub worker_id: String,
    pub max_input_photo: u64,
    pub max_input_video: u64,
}

impl ApiConfig {
    /// `worker_id` is resolved by the caller (its default is hostname plus a random suffix).
    pub fn from_settings(settings: &WorkerSettings, worker_id: String) -> Self {
        Self {
            base_url: settings.worker_api_url.clone(),
            token: settings.worker_api_token.clone(),
            worker_id,
            max_input_photo: settings.worker_max_input_photo.bytes(),
            max_input_video: settings.worker_max_input_video.bytes(),
        }
    }
}

/// Client of the worker routes. The bearer and `X-Worker-Id` are default headers of the one HTTP client, so
/// every request carries them; the bearer is marked sensitive and is never part of a log line, a `Debug` text
/// or an error value. Redirects are not followed.
pub struct WorkerApiClient {
    http: Client,
    base: String,
    max_input_photo: u64,
    max_input_video: u64,
}

/// Which statuses a route documents besides the common 403 and 5xx.
#[derive(Clone, Copy)]
enum Call {
    /// Claim, status and the clock offset: 403 only.
    Unleased,
    /// Lease, release, result and failure: 409.
    Leased,
    /// Input: 404, 409.
    Input,
    /// Output: 404, 409, 413, 422.
    Output,
}

impl WorkerApiClient {
    pub fn new(config: ApiConfig) -> Result<Self, ApiConfigError> {
        let base = config.base_url.trim_end_matches('/').to_owned();
        let url = reqwest::Url::parse(&base).map_err(|_| ApiConfigError::Url)?;
        if url.scheme() != "http" {
            return Err(ApiConfigError::Url);
        }
        let mut bearer = HeaderValue::from_str(&format!("Bearer {}", config.token.expose()))
            .map_err(|_| ApiConfigError::Token)?;
        bearer.set_sensitive(true);
        let worker_id =
            HeaderValue::from_str(&config.worker_id).map_err(|_| ApiConfigError::WorkerId)?;
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, bearer);
        headers.insert(WORKER_ID_HEADER, worker_id);
        let http = Client::builder()
            .tls_backend_preconfigured(no_server_tls()?)
            .default_headers(headers)
            .redirect(redirect::Policy::none())
            .build()
            .map_err(|error| ApiConfigError::Client(error.without_url().to_string()))?;
        Ok(Self {
            http,
            base,
            max_input_photo: config.max_input_photo,
            max_input_video: config.max_input_video,
        })
    }

    /// `POST /claims`: 200 gives the job, 204 means nothing is due.
    pub async fn claim(&self, lane: Lane) -> Result<Option<ClaimedJob>, ApiError> {
        let request =
            self.json_request(self.http.post(self.url("/claims")), &ClaimBody { lane })?;
        let response = self.send(request).await?;
        match response.status() {
            StatusCode::OK => read_json(response).await.map(Some),
            StatusCode::NO_CONTENT => Ok(None),
            status => Err(status_error(status, Call::Unleased)),
        }
    }

    /// `PUT /{job_id}/lease`: the lease is extended; the answer carries the new `lease_ms`.
    pub async fn heartbeat(&self, job_id: Uuid) -> Result<LeaseExtended, ApiError> {
        let response = self
            .send(self.http.put(self.url(&format!("/{job_id}/lease"))))
            .await?;
        read_json(expect_status(response, StatusCode::OK, Call::Leased)?).await
    }

    /// `DELETE /{job_id}/lease`.
    pub async fn release(&self, job_id: Uuid) -> Result<(), ApiError> {
        let response = self
            .send(self.http.delete(self.url(&format!("/{job_id}/lease"))))
            .await?;
        expect_status(response, StatusCode::NO_CONTENT, Call::Leased).map(drop)
    }

    /// `PUT /{job_id}/result`: closes the job without an output.
    pub async fn complete(&self, job_id: Uuid, outcome: SkipOutcome) -> Result<(), ApiError> {
        let request = self.json_request(
            self.http.put(self.url(&format!("/{job_id}/result"))),
            &ResultBody { outcome },
        )?;
        let response = self.send(request).await?;
        expect_status(response, StatusCode::NO_CONTENT, Call::Leased).map(drop)
    }

    /// `POST /{job_id}/failure`: the API answers whether the job is requeued or dead.
    pub async fn fail(&self, job_id: Uuid, report: &FailureReport) -> Result<FailResult, ApiError> {
        let request = self.json_request(
            self.http.post(self.url(&format!("/{job_id}/failure"))),
            report,
        )?;
        let response = self.send(request).await?;
        let body: FailBody =
            read_json(expect_status(response, StatusCode::OK, Call::Leased)?).await?;
        Ok(body.result)
    }

    /// `PUT /status`.
    pub async fn report_status(&self, status: &WorkerStatus) -> Result<(), ApiError> {
        let request = self.json_request(self.http.put(self.url("/status")), status)?;
        let response = self.send(request).await?;
        expect_status(response, StatusCode::NO_CONTENT, Call::Unleased).map(drop)
    }

    fn url(&self, tail: &str) -> String {
        format!("{}{API_PREFIX}{tail}", self.base)
    }

    fn json_request(
        &self,
        request: RequestBuilder,
        body: &impl Serialize,
    ) -> Result<RequestBuilder, ApiError> {
        let bytes = serde_json::to_vec(body)
            .map_err(|_| ApiError::Protocol("the request body does not serialise".into()))?;
        Ok(request.header(CONTENT_TYPE, JSON).body(bytes))
    }

    async fn send(&self, request: RequestBuilder) -> Result<Response, ApiError> {
        request.send().await.map_err(|error| transient(&error))
    }
}

#[derive(Serialize)]
struct ClaimBody {
    lane: Lane,
}

#[derive(Serialize)]
struct ResultBody {
    outcome: SkipOutcome,
}

#[derive(Deserialize)]
struct FailBody {
    result: FailResult,
}

/// A transport error as `Transient`. Only its class is kept: the reqwest text names the URL.
fn transient(error: &reqwest::Error) -> ApiError {
    let class = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connection failed"
    } else if error.is_body() || error.is_decode() {
        "body interrupted"
    } else {
        "request failed"
    };
    ApiError::Transient(class.into())
}

fn expect_status(response: Response, ok: StatusCode, call: Call) -> Result<Response, ApiError> {
    match response.status() {
        status if status == ok => Ok(response),
        status => Err(status_error(status, call)),
    }
}

fn status_error(status: StatusCode, call: Call) -> ApiError {
    use Call::{Input, Leased, Output, Unleased};
    match (status, call) {
        (StatusCode::FORBIDDEN, _) => ApiError::Forbidden,
        (StatusCode::CONFLICT, Leased | Input | Output) => ApiError::LeaseLost,
        (StatusCode::NOT_FOUND, Input | Output) => ApiError::Gone,
        (StatusCode::PAYLOAD_TOO_LARGE, Output) => ApiError::TooLarge,
        (StatusCode::UNPROCESSABLE_ENTITY, Output) => ApiError::TypeRejected,
        (status, Unleased | Leased | Input | Output) if status.is_server_error() => {
            ApiError::Transient(format!("status {}", status.as_u16()))
        }
        (status, _) => ApiError::Protocol(format!("status {}", status.as_u16())),
    }
}

async fn read_json<T: for<'de> Deserialize<'de>>(response: Response) -> Result<T, ApiError> {
    let bytes = response.bytes().await.map_err(|error| transient(&error))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::Protocol("the response body does not parse".into()))
}
