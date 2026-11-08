use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::MethodRouter;
use sc_clock::Clock;
use sc_error::{
    BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode, FieldProblem,
};

use super::client::{ClientVersion, Semver};
use super::minimum::Minimum;
use crate::auth::CredentialFormat;
use crate::auth::bearer;
use crate::config::HttpConfig;
use crate::error::HttpServerError;
use crate::metrics::{ClientVersionLabels, HttpMetrics};
use crate::problem::ApiProblem;
use crate::registry::AuthClass;

/// The request header every native client sends and the bundled web app sets to `web/<version>`.
pub const CLIENT_HEADER: &str = "x-sunnycloud-client";
const HEADER: HeaderName = HeaderName::from_static(CLIENT_HEADER);

/// Why the gate refuses a request.
#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    /// The header is present but is not `<family>/<semver>` (or is repeated).
    Malformed,
    /// No header, and the request is not from a browser session.
    Missing,
    TooOld(Semver),
}

/// Layer 7 of the stack of 11 section 5: the client version gate (AC-64), applied to every route whose auth
/// class is not `Public`, in front of that route's auth.
pub struct VersionGate {
    minimum: Option<Minimum>,
    update_url: Option<String>,
    credentials: CredentialFormat,
    metrics: Arc<HttpMetrics>,
    clock: Arc<dyn Clock>,
}

impl VersionGate {
    /// `InvalidConfig` when `min_client_version` is not one semver or `family=semver,...`, when a minimum is
    /// set without an `update_url`, or when `server_info` advertises another minimum than the gate enforces.
    pub fn new(
        config: &HttpConfig,
        metrics: Arc<HttpMetrics>,
        clock: Arc<dyn Clock>,
    ) -> Result<Arc<VersionGate>, HttpServerError> {
        let invalid = |reason: String| Err(HttpServerError::InvalidConfig(reason));
        let minimum = match config.min_client_version.as_deref() {
            None => None,
            Some(text) => match Minimum::parse(text) {
                Ok(minimum) => Some(minimum),
                Err(reason) => return invalid(format!("min_client_version: {reason}")),
            },
        };
        if let Some(text) = config.min_client_version.as_deref() {
            if config.server_info.min_client_version != text.trim() {
                return invalid(
                    "server_info.min_client_version differs from min_client_version".to_owned(),
                );
            }
            if config
                .update_url
                .as_deref()
                .is_none_or(|url| !is_http_url(url))
            {
                return invalid(
                    "update_url must be an http(s) URL when min_client_version is set".to_owned(),
                );
            }
        }
        Ok(Arc::new(VersionGate {
            minimum,
            update_url: config.update_url.clone(),
            credentials: config.credentials.clone(),
            metrics,
            clock,
        }))
    }

    /// Puts the gate in front of `handler` (and so in front of its auth) unless the route is `Public`. Like
    /// auth, it runs only for a request that matched the route and before any body byte is read.
    pub(crate) fn guard(self: &Arc<Self>, handler: MethodRouter, class: AuthClass) -> MethodRouter {
        if class == AuthClass::Public {
            return handler;
        }
        handler.route_layer(from_fn_with_state(Arc::clone(self), check))
    }

    /// Counts a valid header once, by family and support.
    fn judge(&self, headers: &HeaderMap) -> Result<(), Refusal> {
        let mut values = headers.get_all(&HEADER).iter();
        let Some(first) = values.next() else {
            return self.judge_absent(headers);
        };
        let Some(client) = values
            .next()
            .is_none()
            .then(|| first.to_str().ok().and_then(ClientVersion::parse))
            .flatten()
        else {
            return Err(Refusal::Malformed);
        };
        let required = self
            .minimum
            .as_ref()
            .and_then(|minimum| minimum.for_family(client.family));
        let supported = required.is_none_or(|minimum| client.version >= minimum);
        self.metrics.client_versions.inc(ClientVersionLabels {
            client: client.family,
            supported,
        });
        match required {
            Some(minimum) if !supported => Err(Refusal::TooOld(minimum)),
            _ => Ok(()),
        }
    }

    /// With no minimum configured there is nothing to enforce. Otherwise a missing header passes only for a
    /// browser session: a request with a bearer token is a native one, and a request with neither is not
    /// from the web app.
    fn judge_absent(&self, headers: &HeaderMap) -> Result<(), Refusal> {
        if self.minimum.is_none()
            || (bearer(headers).is_none() && self.credentials.has_session_cookie(headers))
        {
            Ok(())
        } else {
            Err(Refusal::Missing)
        }
    }

    fn problem(&self, refusal: Refusal, request_id: CorrelationId) -> ApiProblem {
        let header_problem = |problem: &str| {
            ApiProblem::new(
                DomainError::Validation(vec![FieldProblem {
                    field: CLIENT_HEADER.to_owned(),
                    problem: problem.to_owned(),
                }]),
                request_id,
            )
        };
        match refusal {
            Refusal::Malformed => header_problem("malformed"),
            Refusal::Missing => header_problem("required"),
            Refusal::TooOld(minimum) => {
                let error = BusinessError::new(
                    ErrorCode::UpgradeRequired,
                    BusinessParams {
                        min_version: Some(minimum.to_string()),
                        ..BusinessParams::default()
                    },
                );
                let problem = ApiProblem::new(DomainError::Business(error), request_id);
                match &self.update_url {
                    Some(url) => problem.with_update_url(url.clone()),
                    None => problem,
                }
            }
        }
    }
}

fn is_http_url(url: &str) -> bool {
    ["https://", "http://"].iter().any(|scheme| {
        url.strip_prefix(scheme)
            .is_some_and(|rest| !rest.is_empty())
    })
}

async fn check(State(gate): State<Arc<VersionGate>>, request: Request, next: Next) -> Response {
    let Err(refusal) = gate.judge(request.headers()) else {
        return next.run(request).await;
    };
    let request_id = request
        .extensions()
        .get::<CorrelationId>()
        .copied()
        .unwrap_or_else(|| CorrelationId::new(sc_ids::new_id(gate.clock.as_ref())));
    gate.problem(refusal, request_id).into_response()
}
