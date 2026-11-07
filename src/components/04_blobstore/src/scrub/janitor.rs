//! `ScrubJanitor`: the scheduled scrub. Each cycle is a `Due` run at the default rate, stepped until it is done
//! or `scrub_cycle_budget` is spent, then finished. Nothing is carried to the next cycle: the next run recomputes
//! its set from `last_scrubbed_at`.

use std::sync::Arc;
use std::time::Duration;

use contracts::{CycleStats, Janitor, JanitorError, ScrubMode, Scrubber, async_trait};
use sc_clock::Timestamp;
use sc_observability::{log_error, log_info, log_warn};

use super::BlobScrubber;
use crate::config::BlobstoreConfig;

/// What one cycle did. `done` is false when the budget ended the cycle with blobs left; they are the next
/// cycle's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrubCycle {
    pub checked: u64,
    pub mismatched: u64,
    pub missing: u64,
    pub done: bool,
}

pub struct ScrubJanitor {
    scrubber: Arc<BlobScrubber>,
    budget: Duration,
}

fn elapsed_ms(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

impl ScrubJanitor {
    pub fn new(config: &BlobstoreConfig, scrubber: Arc<BlobScrubber>) -> Self {
        ScrubJanitor {
            scrubber,
            budget: config.scrub_cycle_budget,
        }
    }

    /// `now` is the start of the run. The budget is measured on the clock's monotonic time and checked between
    /// steps; a step that is paused by the activity gauge also ends at the budget, with a WARN.
    pub async fn run_cycle(&self, now: Timestamp) -> Result<ScrubCycle, JanitorError> {
        log_info!("scrub cycle started");
        let clock = self.scrubber.clock();
        let began = clock.monotonic();
        let deadline = began.checked_add(self.budget).unwrap_or(began);
        let mut cycle = ScrubCycle::default();
        let mut failure = None;
        match self.scrubber.start_run_at(ScrubMode::Due, None, now).await {
            Err(error) => failure = Some(error),
            Ok(run) => {
                loop {
                    match self.scrubber.step_until(&run, Some(deadline)).await {
                        Ok(outcome) => {
                            cycle.checked += outcome.result.checked;
                            cycle.mismatched += outcome.result.mismatched;
                            cycle.missing += outcome.result.missing;
                            cycle.done = outcome.result.done;
                            if outcome.paused_out {
                                log_warn!("scrub paused until the cycle budget ended");
                            }
                            if cycle.done || outcome.paused_out || clock.monotonic() >= deadline {
                                break;
                            }
                        }
                        Err(error) => {
                            failure = Some(error);
                            break;
                        }
                    }
                }
                if let Err(error) = self.scrubber.finish_run(&run).await {
                    failure = failure.or(Some(error));
                }
            }
        }
        if let Some(error) = failure {
            log_error!("scrub cycle failed", err = &error);
            return Err(JanitorError::Unexpected(Box::new(error)));
        }
        let elapsed = clock.monotonic().saturating_duration_since(began);
        log_info!("scrub cycle finished", duration_ms = elapsed_ms(elapsed));
        Ok(cycle)
    }
}

#[async_trait]
impl Janitor for ScrubJanitor {
    fn name(&self) -> &'static str {
        "scrub_janitor"
    }

    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError> {
        let cycle = ScrubJanitor::run_cycle(self, now).await?;
        Ok(CycleStats {
            processed: cycle.checked,
        })
    }
}
