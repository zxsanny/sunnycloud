//! `scrub run` and `scrub status` (E-22/c12): `Scrubber::start_run`, `run_step` until `done`, one `finish_run`;
//! the counts are the outcome and any mismatch or missing blob makes it a finding (exit 2). The rate is the CLI's
//! only throttle (it cannot see the API's activity gauge).

use std::time::Duration;

use contracts::{ByteRate, ScrubError, ScrubMode, ScrubRun, Scrubber, StepResult};
use sc_error::ErrorCode;
use sc_observability::log_error;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;

use crate::context::ServiceContext;
use crate::error::CliError;
use crate::exit::Exit;
use crate::handler::Call;
use crate::outcome::{Class, Outcome};
use crate::settings::CliSettings;

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Default, Clone, Copy)]
struct Totals {
    checked: u64,
    mismatched: u64,
    missing: u64,
}

impl Totals {
    fn add(&mut self, step: StepResult) {
        self.checked = self.checked.saturating_add(step.checked);
        self.mismatched = self.mismatched.saturating_add(step.mismatched);
        self.missing = self.missing.saturating_add(step.missing);
    }

    fn findings(self) -> bool {
        self.mismatched > 0 || self.missing > 0
    }
}

pub(crate) async fn run(
    full: bool,
    rate_mib: Option<u32>,
    call: &Call<'_>,
) -> Result<Outcome, CliError> {
    let scrubber = call.services.get::<dyn Scrubber>().await?;
    let progress = call.services.get::<CliSettings>().await?.progress_interval;
    let mode = if full {
        ScrubMode::Full
    } else {
        ScrubMode::Due
    };
    let rate = rate_mib.map(|mib| ByteRate(u64::from(mib) * MIB));
    let run = scrubber.start_run(mode, rate).await.map_err(failure)?;

    let stepped = step_loop(scrubber.as_ref(), &run, progress, &call.cancel, call).await;
    // The closing call runs after a failed or an interrupted loop too, so the summary is not lost.
    let finished = scrubber.finish_run(&run).await;
    let (totals, interrupted) = match stepped {
        Ok(done) => done,
        Err(error) => {
            if let Err(closing) = finished {
                log_error!(
                    "scrub run: the closing call failed after a failed step",
                    err = &closing
                );
            }
            return Err(error);
        }
    };
    finished.map_err(failure)?;
    let class = if totals.findings() {
        Class::Findings
    } else if interrupted {
        Class::Interrupted
    } else {
        Class::Success
    };
    Ok(Outcome::new(class)
        .field("checked", totals.checked)
        .field("mismatched", totals.mismatched)
        .field("missing", totals.missing)
        .field("interrupted", interrupted))
}

/// Steps until `done` or until `cancel`; a step that runs finishes first. Returns the totals and whether the loop
/// was cut short.
async fn step_loop(
    scrubber: &dyn Scrubber,
    run: &ScrubRun,
    progress: Duration,
    cancel: &CancellationToken,
    call: &Call<'_>,
) -> Result<(Totals, bool), CliError> {
    let mut totals = Totals::default();
    let mut ticker = interval(progress);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await;
    loop {
        if cancel.is_cancelled() {
            return Ok((totals, true));
        }
        let step = scrubber.run_step(run);
        tokio::pin!(step);
        let result = loop {
            tokio::select! {
                result = &mut step => break result.map_err(failure)?,
                _ = ticker.tick() => report(call, totals),
            }
        };
        totals.add(result);
        report(call, totals);
        if result.done {
            return Ok((totals, false));
        }
    }
}

fn report(call: &Call<'_>, totals: Totals) {
    let line = format!(
        "scrub: checked {}, mismatched {}, missing {}\n",
        totals.checked, totals.mismatched, totals.missing
    );
    if let Err(error) = call.terminal.write_stderr(&line) {
        log_error!("scrub run: cannot write the progress line", err = &error);
    }
}

pub(crate) async fn status(services: &ServiceContext) -> Result<Outcome, CliError> {
    let status = services
        .get::<dyn Scrubber>()
        .await?
        .status()
        .await
        .map_err(failure)?;
    let summary = status.latest_summary;
    Ok(Outcome::success()
        .field(
            "oldest_scrubbed_at_micros",
            status.oldest_scrubbed_at.map(|at| at.as_unix_micros()),
        )
        .field("unscrubbed", status.unscrubbed)
        .field("unavailable", status.unavailable)
        .field("last_run_checked", summary.map(|run| run.checked))
        .field("last_run_mismatched", summary.map(|run| run.mismatched))
        .field("last_run_missing", summary.map(|run| run.missing))
        .field(
            "last_run_duration_ms",
            summary.map(|run| u64::try_from(run.duration.as_millis()).unwrap_or(u64::MAX)),
        ))
}

fn failure(error: ScrubError) -> CliError {
    CliError::Operator {
        code: ErrorCode::Internal.as_str(),
        detail: format!("scrub: {error}"),
        exit: Exit::Failed,
    }
}
