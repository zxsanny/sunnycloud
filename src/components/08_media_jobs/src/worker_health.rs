//! Worker presence for `doctor` and the admin health page (AC-63). The `worker_status` row the supervisor
//! refreshes every `worker_status_interval` is the only worker heartbeat: both read the table only and never
//! contact a supervisor. A row counts while it was seen within three intervals.

use std::sync::Arc;
use std::time::Duration;

use contracts::{
    CheckFuture, DoctorCheck, DoctorContext, DoctorTag, Finding, FindingLevel, HealthContributor,
    HealthItem, HealthStatus, async_trait,
};
use sc_clock::Clock;
use sc_db::Db;
use sc_observability::log_error;

use crate::admin_repo::WorkerFacts;
use crate::config::MediaConfig;
use crate::job_repo;
use crate::retry;

/// A decoder image older than this many days is stale (weekly rebuild rule, AC-63).
const MAX_IMAGE_AGE_DAYS: i32 = 14;

/// Status intervals without a report after which a worker counts as absent.
const MISSED_REPORTS: u32 = 3;

/// Registered by 12 as a `DoctorCheck` and as 07's media `HealthContributor`.
pub struct MediaHealth {
    db: Db,
    clock: Arc<dyn Clock>,
    window: Duration,
}

impl MediaHealth {
    pub fn new(db: Db, clock: Arc<dyn Clock>, config: &MediaConfig) -> Self {
        MediaHealth {
            db,
            clock,
            window: config.worker_status_interval.saturating_mul(MISSED_REPORTS),
        }
    }

    async fn facts(&self) -> Option<WorkerFacts> {
        let since = retry::sub(self.clock.now_utc(), self.window);
        match job_repo::worker_facts(&self.db, since).await {
            Ok(facts) => Some(facts),
            Err(error) => {
                log_error!("worker status read failed", err = &error);
                None
            }
        }
    }
}

fn finding(level: FindingLevel, message: String, remedy: &str) -> Finding {
    Finding {
        level,
        message,
        remedy: remedy.to_owned(),
    }
}

fn rank(level: FindingLevel) -> u8 {
    match level {
        FindingLevel::Ok => 0,
        FindingLevel::Warn => 1,
        FindingLevel::Fail => 2,
    }
}

/// The level table of the contract: no fresh row is Warn, or Fail when jobs are queued or dead; a failing decoder
/// is Fail; an image older than 14 days is Warn; otherwise Ok. Both findings of a fresh row are reported.
fn assess(facts: &WorkerFacts, window: Duration) -> Vec<Finding> {
    if facts.fresh == 0 {
        let message = format!(
            "no media worker has reported in the last {}s; {} jobs queued, {} dead",
            window.as_secs(),
            facts.queued,
            facts.dead
        );
        let remedy =
            "start the worker supervisor and check that it reaches the API's internal listener";
        let level = if facts.queued + facts.dead > 0 {
            FindingLevel::Fail
        } else {
            FindingLevel::Warn
        };
        return vec![finding(level, message, remedy)];
    }
    let mut findings = Vec::new();
    if facts.decoder_failing > 0 {
        findings.push(finding(
            FindingLevel::Fail,
            format!(
                "{} of {} media workers report a failing decoder self-check",
                facts.decoder_failing, facts.fresh
            ),
            "check the decoder containers' logs and restart them",
        ));
    }
    if facts.image_age_days > MAX_IMAGE_AGE_DAYS {
        findings.push(finding(
            FindingLevel::Warn,
            format!(
                "the decoder image is {} days old (more than {MAX_IMAGE_AGE_DAYS})",
                facts.image_age_days
            ),
            "pull the current decoder images and restart the worker stack",
        ));
    }
    if findings.is_empty() {
        findings.push(finding(
            FindingLevel::Ok,
            format!(
                "{} media workers reporting; decoder ok; image {} days old",
                facts.fresh, facts.image_age_days
            ),
            "",
        ));
    }
    findings
}

/// The worst finding names the item's status; the detail adds the queue depth and the dead jobs.
fn health_item(findings: &[Finding], facts: &WorkerFacts) -> HealthItem {
    let worst = findings.iter().max_by_key(|finding| rank(finding.level));
    let (status, cause) = match worst {
        Some(finding) => (
            match finding.level {
                FindingLevel::Ok => HealthStatus::Ok,
                FindingLevel::Warn => HealthStatus::Degraded,
                FindingLevel::Fail => HealthStatus::Down,
            },
            finding.message.as_str(),
        ),
        None => (HealthStatus::Down, "no worker finding"),
    };
    HealthItem::new(
        status,
        format!(
            "{cause}; queue depth {}; dead jobs {}",
            facts.queued, facts.dead
        ),
    )
}

#[async_trait]
impl DoctorCheck for MediaHealth {
    fn name(&self) -> &'static str {
        "media_worker"
    }

    /// Neither tag: a restore does not change worker presence, and no worker runs before the stack starts.
    fn tags(&self) -> &[DoctorTag] {
        &[]
    }

    /// A table that cannot be read is one `Fail`: the worker state is unknown.
    async fn run(&self, _ctx: &DoctorContext) -> Vec<Finding> {
        match self.facts().await {
            Some(facts) => assess(&facts, self.window),
            None => vec![finding(
                FindingLevel::Fail,
                "the media worker status could not be read".to_owned(),
                "run doctor again once the database answers",
            )],
        }
    }
}

impl HealthContributor for MediaHealth {
    fn name(&self) -> &str {
        "media"
    }

    /// Not ok exactly when the doctor level is Warn (`Degraded`) or Fail (`Down`). A read that fails is `Down`.
    fn check(&self) -> CheckFuture<'_, HealthItem> {
        Box::pin(async move {
            match self.facts().await {
                Some(facts) => health_item(&assess(&facts, self.window), &facts),
                None => HealthItem::new(HealthStatus::Down, "media worker status read failed"),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_secs(90);

    fn facts(fresh: u64, failing: u64, age: i32, queued: u64, dead: u64) -> WorkerFacts {
        WorkerFacts {
            fresh,
            decoder_failing: failing,
            image_age_days: age,
            queued,
            dead,
        }
    }

    fn worst(findings: &[Finding]) -> FindingLevel {
        findings
            .iter()
            .map(|finding| finding.level)
            .max_by_key(|level| rank(*level))
            .expect("at least one finding")
    }

    #[test]
    fn the_level_table_of_the_contract() {
        // Arrange
        let cases = [
            (facts(0, 0, 0, 0, 0), FindingLevel::Warn),
            (facts(0, 0, 0, 2, 0), FindingLevel::Fail),
            (facts(0, 0, 0, 0, 1), FindingLevel::Fail),
            (facts(1, 0, 3, 5, 5), FindingLevel::Ok),
            (facts(1, 0, 14, 0, 0), FindingLevel::Ok),
            (facts(1, 0, 15, 0, 0), FindingLevel::Warn),
            (facts(2, 1, 3, 0, 0), FindingLevel::Fail),
            (facts(1, 1, 20, 0, 0), FindingLevel::Fail),
        ];

        // Act
        let levels: Vec<FindingLevel> = cases
            .iter()
            .map(|(facts, _)| worst(&assess(facts, WINDOW)))
            .collect();

        // Assert
        let expected: Vec<FindingLevel> = cases.iter().map(|(_, level)| *level).collect();
        assert_eq!(levels, expected);
    }

    #[test]
    fn the_health_item_is_not_ok_exactly_on_warn_or_fail_and_names_the_cause() {
        // Arrange
        let cases = [
            (
                facts(0, 0, 0, 0, 0),
                HealthStatus::Degraded,
                "no media worker",
            ),
            (facts(0, 0, 0, 3, 1), HealthStatus::Down, "no media worker"),
            (facts(1, 0, 3, 3, 1), HealthStatus::Ok, "decoder ok"),
            (facts(1, 0, 15, 0, 0), HealthStatus::Degraded, "15 days old"),
            (facts(1, 1, 3, 0, 0), HealthStatus::Down, "failing decoder"),
        ];

        // Act
        let items: Vec<HealthItem> = cases
            .iter()
            .map(|(facts, ..)| health_item(&assess(facts, WINDOW), facts))
            .collect();

        // Assert
        for (item, (facts, status, cause)) in items.iter().zip(&cases) {
            assert_eq!(item.status, *status, "{item:?}");
            assert!(item.detail.contains(cause), "{item:?}");
            let depth = format!("queue depth {}; dead jobs {}", facts.queued, facts.dead);
            assert!(item.detail.contains(&depth), "{item:?}");
        }
    }
}
