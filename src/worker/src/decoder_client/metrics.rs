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

/// The three families of the decoder client, registered once per process and shared by every slot.
pub struct DecoderMetrics {
    pub(crate) restarts: Counter<SlotName>,
    pub(crate) selfcheck_ok: Gauge<SlotName>,
    build_timestamp: Gauge<SlotName>,
}

impl DecoderMetrics {
    /// Registers `worker_decoder_restarts_total`, `worker_selfcheck_ok` and
    /// `worker_decoder_build_timestamp_seconds`, all labelled by `slot`.
    pub fn register(metrics: &Metrics) -> Result<Arc<Self>, ObsError> {
        Ok(Arc::new(Self {
            restarts: metrics.counter("worker_decoder_restarts_total")?,
            selfcheck_ok: metrics.gauge("worker_selfcheck_ok")?,
            build_timestamp: metrics.gauge("worker_decoder_build_timestamp_seconds")?,
        }))
    }

    /// Records the `build_timestamp` (Unix seconds) the slot's decoder reported in `hello`.
    pub(crate) fn set_build_timestamp(&self, slot: SlotName, unix_seconds: u64) {
        // Protocol range is 1..=4,102,444,800, far below 2^53, so the conversion is exact.
        #[allow(clippy::cast_precision_loss)]
        self.build_timestamp.set(slot, unix_seconds as f64);
    }

    /// The last `build_timestamp` the slot's decoder reported; `None` before its first `hello`.
    pub fn build_timestamp(&self, slot: SlotName) -> Option<u64> {
        let seconds = self.build_timestamp.get(slot);
        // Set only from a `u64`, so the value is a whole number; zero means never set.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (seconds >= 1.0).then_some(seconds as u64)
    }

    pub fn restarts(&self, slot: SlotName) -> u64 {
        self.restarts.get(slot)
    }

    pub fn selfcheck_ok(&self, slot: SlotName) -> bool {
        self.selfcheck_ok.get(slot) >= 1.0
    }
}
