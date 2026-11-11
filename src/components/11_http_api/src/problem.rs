//! The one place a [`DomainError`] becomes an HTTP answer: status from the `sc_error` table, the headers and
//! the `application/problem+json` body of `problem_details.md`.

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use sc_error::{
    BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode, FieldProblem,
};
use sc_observability::log_error;

use crate::port_error::PortError;

const PROBLEM_JSON: &str = "application/problem+json";
const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// A failure ready to be returned from a handler. Mapping is synchronous: nothing awaits between the
/// port's return and the response.
#[derive(Debug)]
pub struct ApiProblem {
    error: DomainError,
    request_id: CorrelationId,
    /// Members spliced into the serialised body: `sc_error` has no field for them, and one may replace a member
    /// of the table's body (the `code` of an RFC 8628 poll answer).
    extras: Vec<(&'static str, String)>,
}

impl ApiProblem {
    pub fn new(error: DomainError, request_id: CorrelationId) -> ApiProblem {
        ApiProblem {
            error,
            request_id,
            extras: Vec::new(),
        }
    }

    /// Adds the `update_url` extra of an `upgrade_required` body (`server_info.md`).
    pub(crate) fn with_update_url(mut self, url: String) -> ApiProblem {
        self.extras.push(("update_url", url));
        self
    }

    /// An RFC 8628 device-poll answer: status 400, and `type`, `title` and `code` are the grant's own words
    /// (`authorization_pending`, ...) because `sc_error` has no row for them (`problem_details.md`, Open point 3).
    /// The RFC's `error` member carries the same word.
    pub(crate) fn device_poll(
        code: &'static str,
        title: &'static str,
        request_id: CorrelationId,
    ) -> ApiProblem {
        let mut problem = ApiProblem::code(ErrorCode::InvalidRequest, request_id);
        problem.extras.extend([
            ("type", format!("urn:sunnycloud:error:{code}")),
            ("title", title.to_owned()),
            ("code", code.to_owned()),
            ("error", code.to_owned()),
        ]);
        problem
    }

    /// A 400 for one field of the request.
    pub(crate) fn validation(field: &str, problem: &str, request_id: CorrelationId) -> ApiProblem {
        let problem = FieldProblem {
            field: field.to_owned(),
            problem: problem.to_owned(),
        };
        ApiProblem::new(DomainError::Validation(vec![problem]), request_id)
    }

    /// A business failure with `code` and no parameters.
    pub(crate) fn code(code: ErrorCode, request_id: CorrelationId) -> ApiProblem {
        let error = BusinessError::new(code, BusinessParams::default());
        ApiProblem::new(DomainError::Business(error), request_id)
    }

    /// The one 404 for a missing, foreign, expired or revoked resource (`BusinessError::not_found`).
    pub(crate) fn not_found(request_id: CorrelationId) -> ApiProblem {
        ApiProblem::new(
            DomainError::Business(BusinessError::not_found()),
            request_id,
        )
    }

    /// A port's own error, converted by [`PortError`].
    pub fn from_port(error: impl PortError, request_id: CorrelationId) -> ApiProblem {
        ApiProblem::new(error.into_domain(request_id), request_id)
    }
}

impl IntoResponse for ApiProblem {
    fn into_response(self) -> Response {
        let ApiProblem {
            error,
            request_id,
            extras,
        } = self;
        if let DomainError::Unexpected { .. } = &error {
            log_error!(
                "unexpected failure answered as internal",
                request_id = request_id,
                err = &error
            );
        }
        let problem = error.problem(request_id);
        let status = status_of(problem.code.status(), request_id);
        let body = match serialise(&problem, extras) {
            Ok(bytes) => bytes,
            Err(failure) => {
                log_error!(
                    "problem body could not be serialised",
                    request_id = request_id,
                    err = &failure
                );
                Vec::new()
            }
        };
        let mut response = Response::new(Body::from(body));
        *response.status_mut() = status;
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(PROBLEM_JSON));
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        insert_request_id(headers, request_id);
        if matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
        ) && let Some(delay) = problem.params.retry_after
        {
            let seconds = delay
                .as_secs()
                .saturating_add(u64::from(delay.subsec_nanos() > 0));
            headers.insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        response
    }
}

fn serialise(
    problem: &sc_error::ProblemBody,
    extras: Vec<(&'static str, String)>,
) -> Result<Vec<u8>, serde_json::Error> {
    if extras.is_empty() {
        return serde_json::to_vec(problem);
    }
    let mut value = serde_json::to_value(problem)?;
    if let Some(object) = value.as_object_mut() {
        for (name, text) in extras {
            object.insert(name.to_owned(), text.into());
        }
    }
    serde_json::to_vec(&value)
}

fn status_of(status: u16, request_id: CorrelationId) -> StatusCode {
    match StatusCode::from_u16(status) {
        Ok(status) => status,
        Err(failure) => {
            log_error!(
                "status table entry is not an HTTP status",
                request_id = request_id,
                err = &failure
            );
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

pub(crate) fn insert_request_id(headers: &mut HeaderMap, request_id: CorrelationId) {
    match HeaderValue::from_str(&request_id.to_string()) {
        Ok(value) => {
            headers.insert(REQUEST_ID, value);
        }
        Err(failure) => log_error!(
            "request id is not a header value",
            request_id = request_id,
            err = &failure
        ),
    }
}
