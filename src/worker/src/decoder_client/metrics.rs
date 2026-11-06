use std::sync::Arc;

use sc_observability::{Counter, Gauge, LabelSet, Metrics, ObsError};

/// Which decoder a series belongs to: the worker has one slot per socket (`worker_sock_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotName {
    Interactive,
    General,
}

impl SlotName {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            SlotName::Interactive => "interactive",
            SlotName::General => "general",
        }
    }

    pub(crate) fn readiness_name(self) -> &'static str {
        match self {
            SlotName::Interactive => "decoder_interactive",
            SlotName::General => "decoder_general",
        }
    }
}

impl LabelSet for SlotName {
    const NAMES: &'static [&'static str] = &["slot"];

    fn value(&self, _index: usize) -> &'static str {
        self.as_str()
    }
}

/// The two families of the decoder client, registered once per process and shared by every slot.
pub struct DecoderMetrics {
    pub(crate) restarts: Counter<SlotName>,
    pub(crate) selfcheck_ok: Gauge<SlotName>,
}

impl DecoderMetrics {
    /// Registers `worker_decoder_restarts_total` and `worker_selfcheck_ok`, both labelled by `slot`.
    pub fn register(metrics: &Metrics) -> Result<Arc<Self>, ObsError> {
        Ok(Arc::new(Self {
            restarts: metrics.counter("worker_decoder_restarts_total")?,
            selfcheck_ok: metrics.gauge("worker_selfcheck_ok")?,
        }))
    }

    pub fn restarts(&self, slot: SlotName) -> u64 {
        self.restarts.get(slot)
    }

    pub fn selfcheck_ok(&self, slot: SlotName) -> bool {
        self.selfcheck_ok.get(slot) >= 1.0
    }
}
