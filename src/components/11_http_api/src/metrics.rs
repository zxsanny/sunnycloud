use sc_observability::{Counter, Gauge, LabelSet, Metrics, NoLabels, ObsError};

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
