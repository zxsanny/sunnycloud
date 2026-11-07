use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{MatchedPath, Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use sc_observability::log_info;

use super::Layers;
use crate::metrics::{DurationLabels, MethodLabel, RequestLabels, StatusClass};

/// The label of a request no registered route matched (an unknown path, the web app, a share URL with no route).
pub(crate) const UNMATCHED: &str = "unmatched";

/// The route templates the server registers. A request is labelled with the template the router matched,
/// looked up here so the label is a string written in the source; nothing taken from the path reaches a
/// metric or a log line, which is what keeps share tokens and ids out of both.
#[derive(Debug, Clone, Default)]
pub(crate) struct RouteLabels(HashSet<&'static str>);

impl RouteLabels {
    pub(crate) fn new(templates: impl IntoIterator<Item = &'static str>) -> RouteLabels {
        RouteLabels(templates.into_iter().collect())
    }

    /// The registered template the router matched, or `None`.
    pub(crate) fn template(&self, matched: Option<&str>) -> Option<&'static str> {
        matched.and_then(|template| self.0.get(template).copied())
    }
}

/// Layer 4. Counts the request by route template, method and status class, times it by route template, and
/// writes one info line for every answer that is not 2xx (none for 2xx). The line holds the route template,
/// the status and the duration; the request id comes from the span of layer 3.
///
/// The time is to the response head: a body still streaming is not included.
pub(super) async fn observe(
    State(layers): State<Arc<Layers>>,
    request: Request,
    next: Next,
) -> Response {
    let method = MethodLabel::of(request.method());
    let matched = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str);
    let template = layers.routes.template(matched);
    let route = template.unwrap_or(UNMATCHED);
    let started = Instant::now();
    let response = next.run(request).await;
    let elapsed = started.elapsed();
    let status = response.status();
    layers.metrics.requests.inc(RequestLabels {
        route,
        method,
        status: StatusClass::of(status),
    });
    layers
        .metrics
        .request_duration
        .observe(DurationLabels { route }, elapsed.as_secs_f64());
    if !status.is_success() {
        log_answer(template, status, elapsed.as_millis());
    }
    response
}

/// `unmatched` is not a template (the log schema takes templates only), so an unmatched request is logged
/// without a route.
fn log_answer(template: Option<&'static str>, status: StatusCode, elapsed_ms: u128) {
    let status = status.as_u16();
    let duration_ms = u64::try_from(elapsed_ms).unwrap_or(u64::MAX);
    match template {
        Some(route) => log_info!(
            "request answered",
            route = route,
            status = status,
            duration_ms = duration_ms
        ),
        None => log_info!(
            "request answered",
            status = status,
            duration_ms = duration_ms
        ),
    }
}

impl MethodLabel {
    pub(crate) fn of(method: &Method) -> MethodLabel {
        match *method {
            Method::GET => MethodLabel::Get,
            Method::HEAD => MethodLabel::Head,
            Method::POST => MethodLabel::Post,
            Method::PUT => MethodLabel::Put,
            Method::PATCH => MethodLabel::Patch,
            Method::DELETE => MethodLabel::Delete,
            Method::OPTIONS => MethodLabel::Options,
            _ => MethodLabel::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE: &str = "/api/v1/nodes/{id}";
    const SHARE: &str = "/api/v1/public-shares/{token}";

    #[test]
    fn a_matched_template_is_the_label_and_the_share_token_never_is() {
        // Arrange
        let labels = RouteLabels::new([NODE, SHARE]);

        // Act
        let share = labels.template(Some(SHARE));
        let raw_path = labels.template(Some("/api/v1/public-shares/s3cr3t-token"));

        // Assert
        assert_eq!(share, Some(SHARE));
        assert_eq!(raw_path, None);
    }

    #[test]
    fn no_match_has_no_template() {
        // Arrange
        let labels = RouteLabels::new([NODE]);

        // Act
        let template = labels.template(None);

        // Assert
        assert_eq!(template, None);
    }

    #[test]
    fn status_classes_follow_the_hundreds_digit() {
        // Arrange
        let codes = [200, 204, 304, 404, 426, 503];

        // Act
        let classes: Vec<_> = codes
            .iter()
            .map(|code| StatusClass::of(StatusCode::from_u16(*code).expect("status")))
            .collect();

        // Assert
        assert_eq!(
            classes,
            [
                StatusClass::Success,
                StatusClass::Success,
                StatusClass::Redirection,
                StatusClass::ClientError,
                StatusClass::ClientError,
                StatusClass::ServerError,
            ]
        );
    }
}
