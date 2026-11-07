use axum::http::StatusCode;
use sc_observability::{Counter, Gauge, Histogram, LabelSet, Metrics, NoLabels, ObsError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RejectReason {
    /// `max_connections` reached.
    Limit,
}

impl LabelSet for RejectReason {
    const NAMES: &'static [&'static str] = &["reason"];

    fn value(&self, _index: usize) -> &'static str {
        match self {
            RejectReason::Limit => "limit",
        }
    }
}

/// The accept-loop families of the 11 metrics list: `connections_open` and `connections_rejected_total`.
pub(crate) struct ConnectionMetrics {
    pub(crate) open: Gauge<NoLabels>,
    pub(crate) rejected: Counter<RejectReason>,
}

impl ConnectionMetrics {
    pub(crate) fn register(metrics: &Metrics) -> Result<Self, ObsError> {
        let open = metrics.gauge::<NoLabels>("connections_open")?;
        let rejected = metrics.counter::<RejectReason>("connections_rejected_total")?;
        // A family without a sample is absent from the scrape; start every series at 0.
        open.set(NoLabels, 0.0);
        rejected.inc_by(RejectReason::Limit, 0);
        Ok(ConnectionMetrics { open, rejected })
    }
}

/// Status class of a response, the only status information a series carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusClass {
    Informational,
    Success,
    Redirection,
    ClientError,
    ServerError,
}

impl StatusClass {
    pub fn of(status: StatusCode) -> StatusClass {
        match status.as_u16() / 100 {
            1 => StatusClass::Informational,
            2 => StatusClass::Success,
            3 => StatusClass::Redirection,
            4 => StatusClass::ClientError,
            _ => StatusClass::ServerError,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            StatusClass::Informational => "1xx",
            StatusClass::Success => "2xx",
            StatusClass::Redirection => "3xx",
            StatusClass::ClientError => "4xx",
            StatusClass::ServerError => "5xx",
        }
    }
}

/// The request method as a bounded enumeration; a method outside the common seven is `other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodLabel {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Other,
}

impl MethodLabel {
    const fn as_str(self) -> &'static str {
        match self {
            MethodLabel::Get => "GET",
            MethodLabel::Head => "HEAD",
            MethodLabel::Post => "POST",
            MethodLabel::Put => "PUT",
            MethodLabel::Patch => "PATCH",
            MethodLabel::Delete => "DELETE",
            MethodLabel::Options => "OPTIONS",
            MethodLabel::Other => "other",
        }
    }
}

/// The labels of `http_requests_total`. `route` is a route template written in the source
/// (`/api/v1/nodes/{id}`), or `unmatched`, never a request path, so the series count is bounded by the route
/// table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestLabels {
    pub route: &'static str,
    pub method: MethodLabel,
    pub status: StatusClass,
}

impl LabelSet for RequestLabels {
    const NAMES: &'static [&'static str] = &["route", "method", "status_class"];

    fn value(&self, index: usize) -> &'static str {
        match index {
            0 => self.route,
            1 => self.method.as_str(),
            _ => self.status.as_str(),
        }
    }
}

/// The label of `http_request_duration_seconds`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationLabels {
    pub route: &'static str,
}

impl LabelSet for DurationLabels {
    const NAMES: &'static [&'static str] = &["route"];

    fn value(&self, _index: usize) -> &'static str {
        self.route
    }
}

/// The HTTP families of the 11 metrics list. Registered here; the request layers increment them
/// (`body_limit_exceeded` is E-21/c05's).
///
/// A labelled family is absent from a scrape until its first sample.
pub struct HttpMetrics {
    pub requests: Counter<RequestLabels>,
    pub request_duration: Histogram<DurationLabels>,
    pub body_limit_exceeded: Counter<NoLabels>,
    /// Handler panics the outermost layer caught.
    pub panics: Counter<NoLabels>,
}

impl HttpMetrics {
    pub fn register(metrics: &Metrics) -> Result<Self, ObsError> {
        let body_limit_exceeded = metrics.counter::<NoLabels>("body_limit_exceeded_total")?;
        body_limit_exceeded.inc_by(NoLabels, 0);
        let panics = metrics.counter::<NoLabels>("http_panics_total")?;
        panics.inc_by(NoLabels, 0);
        Ok(HttpMetrics {
            requests: metrics.counter::<RequestLabels>("http_requests_total")?,
            request_duration: metrics
                .histogram::<DurationLabels>("http_request_duration_seconds")?,
            body_limit_exceeded,
            panics,
        })
    }
}
