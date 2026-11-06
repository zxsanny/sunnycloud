use sc_observability::{Counter, Gauge, LabelSet, Metrics, ObsError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CycleResult {
    Ok,
    Failed,
}

/// `task` is the janitor's `name()`, a `&'static str` written in the janitor's source.
#[derive(Clone, Copy)]
pub(crate) struct CycleLabels {
    pub(crate) task: &'static str,
    pub(crate) result: CycleResult,
}

impl LabelSet for CycleLabels {
    const NAMES: &'static [&'static str] = &["task", "result"];

    fn value(&self, index: usize) -> &'static str {
        match (index, self.result) {
            (0, _) => self.task,
            (_, CycleResult::Ok) => "ok",
            (_, CycleResult::Failed) => "failed",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TaskLabel(pub(crate) &'static str);

impl LabelSet for TaskLabel {
    const NAMES: &'static [&'static str] = &["task"];

    fn value(&self, _index: usize) -> &'static str {
        self.0
    }
}

/// `janitor_cycles_total{task,result}` and `janitor_last_success_timestamp_seconds{task}` (exported with the
/// `sunnycloud_` prefix).
pub struct JanitorMetrics {
    cycles: Counter<CycleLabels>,
    last_success: Gauge<TaskLabel>,
}

impl JanitorMetrics {
    /// `Duplicate` when the families are already registered in `metrics`.
    pub fn new(metrics: &Metrics) -> Result<Self, ObsError> {
        Ok(JanitorMetrics {
            cycles: metrics.counter("janitor_cycles_total")?,
            last_success: metrics.gauge("janitor_last_success_timestamp_seconds")?,
        })
    }

    pub(crate) fn cycle_finished(&self, task: &'static str, result: CycleResult) {
        self.cycles.inc(CycleLabels { task, result });
    }

    pub(crate) fn succeeded_at(&self, task: &'static str, unix_seconds: f64) {
        self.last_success.set(TaskLabel(task), unix_seconds);
    }
}
