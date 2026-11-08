//! `BlobScrubber`: the `Scrubber` port, and `ScrubJanitor`, its scheduled form.
//!
//! A run is a cursor, not a stored job. `start_run` fixes `started_at`; the set of a step is computed from the
//! rows each time (committed, created at or before `started_at`, `last_scrubbed_at` empty or older than the
//! threshold), oldest first, and every checked blob leaves it by getting a newer `last_scrubbed_at`. So a run
//! ends under live traffic, and an interrupted run loses nothing: the next one selects what is left.
//!
//! A step is three phases and none holds a transaction while a file is read: one short transaction selects the
//! batch; each blob is read under the rate cap and the activity gauge; one short transaction records the
//! progress. A finding is re-checked against its row in a new short transaction before it is reported (see
//! [`report`]).

mod janitor;
mod metrics;
mod pace;
mod probe;
mod report;
mod run;
pub(crate) mod scrub_repo;
mod verify;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use contracts::{
    BlobError, ByteRate, ScrubError, ScrubMode, ScrubRun, ScrubStatus, ScrubSummary, Scrubber,
    StepResult, async_trait,
};
use sc_audit::{AuditDetail, AuditEvent, AuditEventKind, AuditReader, AuditWriter, StoredValue};
use sc_clock::{Clock, MonotonicInstant, Timestamp};
use sc_db::Db;
use uuid::Uuid;

use crate::activity::ActivityGauge;
use crate::config::BlobstoreConfig;
use crate::error::{BlobIds, blob_error_from_io, row_fault, run_fault};
use crate::keyring::MasterKeyring;
use crate::reader::ReadProbe;
pub use janitor::{ScrubCycle, ScrubJanitor};
pub use metrics::ScrubMetrics;
#[cfg(feature = "sync-hook")]
pub use probe::{ScrubHook, ScrubPoint};
use report::{Reported, Reporter};
use run::{RunBook, StepRecord, Tally};
use verify::{Files, Gate, Interrupted, Verdict};

/// A step's outcome. `paused_out` is true when the step stopped because the gauge stayed above the limit until
/// the deadline the janitor gave it; no ciphertext was read after that.
pub(crate) struct StepOutcome {
    pub result: StepResult,
    pub paused_out: bool,
}

pub struct BlobScrubber {
    db: Db,
    clock: Arc<dyn Clock>,
    root: PathBuf,
    keyring: Arc<MasterKeyring>,
    reporter: Reporter,
    summaries: AuditReader,
    interval: Duration,
    default_rate: u64,
    pause_above: u64,
    batch: i64,
    gauge: Option<ActivityGauge>,
    metrics: ScrubMetrics,
    runs: RunBook,
    read_probe: Arc<ReadProbe>,
    probe: probe::Probe,
}

fn unknown_run() -> ScrubError {
    ScrubError::Unexpected(row_fault("unknown scrub run", BlobIds::default()))
}

fn db_failure(error: sc_db::RunError<sc_db::DbError>) -> ScrubError {
    ScrubError::Unexpected(run_fault(error, BlobIds::default()))
}

fn int(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

impl BlobScrubber {
    /// `config.blob_root` is required. `batch` is the number of blobs one step takes; the configuration table has
    /// no key for it, so the composition root chooses it. It and `scrub_max_rate` must be at least 1.
    pub fn new(
        config: &BlobstoreConfig,
        keyring: Arc<MasterKeyring>,
        db: Db,
        audit: AuditWriter,
        clock: Arc<dyn Clock>,
        metrics: ScrubMetrics,
        batch: u32,
    ) -> Result<Self, BlobError> {
        let invalid = || {
            blob_error_from_io(
                std::io::Error::from(std::io::ErrorKind::InvalidInput),
                BlobIds::default(),
            )
        };
        let root = config
            .blob_root
            .as_deref()
            .filter(|root| !root.is_empty())
            .ok_or_else(invalid)?;
        if batch == 0 || config.scrub_max_rate == 0 {
            return Err(invalid());
        }
        Ok(BlobScrubber {
            root: PathBuf::from(root),
            keyring,
            reporter: Reporter {
                db: db.clone(),
                audit,
                clock: Arc::clone(&clock),
                root: PathBuf::from(root),
            },
            summaries: AuditReader::new(db.clone()),
            db,
            clock,
            interval: config.scrub_interval,
            default_rate: config.scrub_max_rate,
            pause_above: config.scrub_pause_active_streams,
            batch: i64::from(batch),
            gauge: None,
            metrics,
            runs: RunBook::default(),
            read_probe: Arc::default(),
            probe: probe::Probe::new(),
        })
    }

    /// The API process passes the gauge its writer and reader count into; a CLI run has none and is throttled
    /// by its rate only.
    pub fn with_activity(mut self, gauge: ActivityGauge) -> Self {
        self.gauge = Some(gauge);
        self
    }

    #[cfg(feature = "sync-hook")]
    pub fn with_hook(mut self, hook: Arc<dyn ScrubHook>) -> Self {
        self.probe.hook = Some(hook);
        self
    }

    /// The ciphertext reads of this scrubber; the test hook of the pause and of the rate.
    #[cfg(feature = "sync-hook")]
    pub fn read_probe(&self) -> Arc<ReadProbe> {
        Arc::clone(&self.read_probe)
    }

    pub(crate) fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// `start_run` with the start given: the scheduled cycle starts its run at the `now` the scheduler passes.
    pub(crate) async fn start_run_at(
        &self,
        mode: ScrubMode,
        rate: Option<ByteRate>,
        started_at: Timestamp,
    ) -> Result<ScrubRun, ScrubError> {
        let rate = rate.map_or(self.default_rate, |rate| rate.0);
        let threshold = match mode {
            ScrubMode::Full => Some(started_at),
            ScrubMode::Due => started_at.checked_sub(self.interval),
        };
        let (true, Some(threshold)) = (rate > 0, threshold) else {
            return Err(ScrubError::Unexpected(row_fault(
                "scrub rate or interval out of range",
                BlobIds::default(),
            )));
        };
        self.runs.start(
            started_at,
            threshold.as_unix_micros(),
            rate,
            self.clock.monotonic(),
        );
        self.metrics.run_started();
        Ok(ScrubRun { started_at })
    }

    /// One step. A `deadline` ends a pause that lasts past it; without one a paused step waits for the gauge.
    pub(crate) async fn step_until(
        &self,
        run: &ScrubRun,
        deadline: Option<MonotonicInstant>,
    ) -> Result<StepOutcome, ScrubError> {
        let plan = self.runs.plan(run.started_at).ok_or_else(unknown_run)?;
        let mut pacer = plan.pacer;
        let mut set_aside = plan.set_aside;
        let batch = scrub_repo::select(
            &self.db,
            run.started_at.as_unix_micros(),
            plan.threshold_micros,
            &set_aside,
            self.batch,
        )
        .await
        .map_err(db_failure)?;
        if batch.is_empty() {
            return Ok(StepOutcome {
                result: StepResult {
                    checked: 0,
                    mismatched: 0,
                    missing: 0,
                    done: true,
                },
                paused_out: false,
            });
        }
        let files = Files {
            root: &self.root,
            keyring: &self.keyring,
            probe: &self.read_probe,
        };
        let mut tally = Tally::default();
        let mut scrubbed: Vec<Uuid> = Vec::with_capacity(batch.len());
        let mut slept = Duration::ZERO;
        let mut paused_out = false;
        let mut failure = None;
        for candidate in &batch {
            self.probe
                .at(probe::ScrubPoint::Opening(candidate.id))
                .await;
            let mut gate = Gate {
                clock: &*self.clock,
                gauge: self.gauge.as_ref(),
                pause_above: self.pause_above,
                deadline,
                pacer: &mut pacer,
                slept: Duration::ZERO,
            };
            let verdict = files.verify(candidate, &mut gate).await;
            slept += gate.slept;
            let verdict = match verdict {
                Ok(verdict) => verdict,
                Err(Interrupted::PausedPastDeadline) => {
                    paused_out = true;
                    break;
                }
                Err(Interrupted::Cancelled) => {
                    failure = Some(ScrubError::Unexpected(row_fault(
                        "clock shut down",
                        BlobIds::default(),
                    )));
                    break;
                }
                Err(Interrupted::Failed(error)) => {
                    failure = Some(ScrubError::Unexpected(Box::new(error)));
                    break;
                }
            };
            self.probe
                .at(probe::ScrubPoint::Verified(candidate.id))
                .await;
            if verdict == Verdict::Clean {
                tally.checked += 1;
                scrubbed.push(candidate.id);
                continue;
            }
            match self.reporter.report(candidate.id, verdict).await {
                Ok(Reported::Counted) => {
                    tally.checked += 1;
                    if verdict == Verdict::Corrupt {
                        tally.mismatched += 1;
                    } else {
                        tally.missing += 1;
                    }
                    scrubbed.push(candidate.id);
                }
                Ok(Reported::Deferred) => set_aside.push(candidate.id),
                Ok(Reported::NotCommitted) => {}
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        let progress = if scrubbed.is_empty() {
            Ok(())
        } else {
            let at = self.clock.now_utc().as_unix_micros();
            scrub_repo::mark_scrubbed(&self.db, &scrubbed, at)
                .await
                .map_err(db_failure)
        };
        self.metrics
            .step_done(tally.checked, tally.mismatched, tally.missing);
        self.metrics.throttled(slept);
        self.runs.record(
            run.started_at,
            StepRecord {
                pacer,
                set_aside,
                tally,
            },
        );
        progress?;
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(StepOutcome {
            result: StepResult {
                checked: tally.checked,
                mismatched: tally.mismatched,
                missing: tally.missing,
                done: false,
            },
            paused_out,
        })
    }

    async fn latest_summary(&self) -> Result<Option<ScrubSummary>, ScrubError> {
        let mut cursor = None;
        let mut latest = None;
        loop {
            let page = self
                .summaries
                .export(
                    Timestamp::from_unix_micros(0),
                    Some(AuditEventKind::ScrubSummary),
                    cursor,
                )
                .await
                .map_err(|error| ScrubError::Unexpected(Box::new(error)))?;
            if let Some(row) = page.rows.into_iter().last() {
                latest = Some(row);
            }
            match page.next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        latest
            .map(|row| {
                let detail = row.detail.unwrap_or_default();
                let count = |key: &str| {
                    detail.iter().find_map(|(name, value)| match value {
                        StoredValue::Int(count) if name == key => u64::try_from(*count).ok(),
                        _ => None,
                    })
                };
                let malformed = || {
                    ScrubError::Unexpected(row_fault("malformed scrub summary", BlobIds::default()))
                };
                Ok(ScrubSummary {
                    checked: count("checked").ok_or_else(malformed)?,
                    mismatched: count("mismatched").ok_or_else(malformed)?,
                    missing: count("missing").ok_or_else(malformed)?,
                    duration: Duration::from_millis(count("duration_ms").ok_or_else(malformed)?),
                })
            })
            .transpose()
    }
}

#[async_trait]
impl Scrubber for BlobScrubber {
    async fn start_run(
        &self,
        mode: ScrubMode,
        rate: Option<ByteRate>,
    ) -> Result<ScrubRun, ScrubError> {
        self.start_run_at(mode, rate, self.clock.now_utc()).await
    }

    async fn run_step(&self, run: &ScrubRun) -> Result<StepResult, ScrubError> {
        self.step_until(run, None)
            .await
            .map(|outcome| outcome.result)
    }

    async fn finish_run(&self, run: &ScrubRun) -> Result<(), ScrubError> {
        let (tally, began) = self.runs.finish(run.started_at).ok_or_else(unknown_run)?;
        let duration = self.clock.monotonic().saturating_duration_since(began);
        let event = AuditEvent {
            detail: AuditDetail::new()
                .with_int("checked", int(tally.checked))
                .with_int("mismatched", int(tally.mismatched))
                .with_int("missing", int(tally.missing))
                .with_int(
                    "duration_ms",
                    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
                ),
            ..AuditEvent::new(AuditEventKind::ScrubSummary)
        };
        self.reporter.audit.record_best_effort(event).await;
        self.metrics.run_finished(self.clock.now_utc(), duration);
        Ok(())
    }

    async fn status(&self) -> Result<ScrubStatus, ScrubError> {
        let counts = scrub_repo::counts(&self.db).await.map_err(db_failure)?;
        Ok(ScrubStatus {
            oldest_scrubbed_at: counts
                .oldest_scrubbed_micros
                .map(Timestamp::from_unix_micros),
            unscrubbed: u64::try_from(counts.unscrubbed).unwrap_or(0),
            latest_summary: self.latest_summary().await?,
            unavailable: u64::try_from(counts.unavailable).unwrap_or(0),
        })
    }
}
