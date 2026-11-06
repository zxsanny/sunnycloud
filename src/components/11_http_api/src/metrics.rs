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

/// The labels of the request families. `route` is a route template written in the source (`/api/v1/nodes/{id}`),
/// never a request path, so the series count is bounded by the route table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestLabels {
    pub route: &'static str,
    pub status: StatusClass,
}

impl LabelSet for RequestLabels {
    const NAMES: &'static [&'static str] = &["route", "status"];

    fn value(&self, index: usize) -> &'static str {
        match (index, self.status) {
            (0, _) => self.route,
            (_, StatusClass::Informational) => "1xx",
            (_, StatusClass::Success) => "2xx",
            (_, StatusClass::Redirection) => "3xx",
            (_, StatusClass::ClientError) => "4xx",
            (_, StatusClass::ServerError) => "5xx",
        }
    }
}

/// The HTTP families of the 11 metrics list. Registered here; the request middleware and the body-limit
/// layer (E-21/c04, c05) increment them.
///
/// A labelled family is absent from a scrape until its first sample.
pub struct HttpMetrics {
    pub requests: Counter<RequestLabels>,
    pub request_duration: Histogram<RequestLabels>,
    pub body_limit_exceeded: Counter<NoLabels>,
}

impl HttpMetrics {
    pub(crate) fn register(metrics: &Metrics) -> Result<Self, ObsError> {
        let body_limit_exceeded = metrics.counter::<NoLabels>("body_limit_exceeded_total")?;
        body_limit_exceeded.inc_by(NoLabels, 0);
        Ok(HttpMetrics {
            requests: metrics.counter::<RequestLabels>("http_requests_total")?,
            request_duration: metrics
                .histogram::<RequestLabels>("http_request_duration_seconds")?,
            body_limit_exceeded,
        })
    }
}
