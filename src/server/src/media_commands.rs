//! `admin media-requeue` and `admin media-stats` (E-22/c13): one call on 08's `MediaAdmin` each. The filters are
//! validated before any service is built, so a bad argument exits 3 with no call.

use contracts::{JobKind, JobStatus, Lane, MediaAdmin, MediaAdminError, MediaStats, RequeueFilter};
use sc_error::ErrorCode;
use uuid::Uuid;

use crate::context::ServiceContext;
use crate::error::CliError;
use crate::exit::Exit;
use crate::outcome::Outcome;

/// The text forms of 08's job kinds, as its `jobs` table and metrics label them.
const KINDS: [(&str, JobKind); 3] = [
    ("stills", JobKind::Stills),
    ("poster", JobKind::Poster),
    ("transcode", JobKind::Transcode),
];
const LANES: [(&str, Lane); 2] = [("image", Lane::Image), ("video", Lane::Video)];
const STATUSES: [JobStatus; 4] = [
    JobStatus::Queued,
    JobStatus::Running,
    JobStatus::Succeeded,
    JobStatus::Dead,
];

pub(crate) async fn requeue(
    account: Option<&str>,
    kind: Option<&str>,
    services: &ServiceContext,
) -> Result<Outcome, CliError> {
    let filter = filter(account, kind)?;
    let admin = services.get::<dyn MediaAdmin>().await?;
    let requeued = admin.requeue_dead(filter).await.map_err(failure)?;
    Ok(Outcome::success().field("requeued", requeued))
}

pub(crate) async fn stats(services: &ServiceContext) -> Result<Outcome, CliError> {
    let admin = services.get::<dyn MediaAdmin>().await?;
    admin.stats().await.map(depths).map_err(failure)
}

fn filter(account: Option<&str>, kind: Option<&str>) -> Result<RequeueFilter, CliError> {
    let account = account
        .map(|text| {
            Uuid::parse_str(text)
                .map_err(|_| CliError::Usage(format!("--account `{text}` is not an account id")))
        })
        .transpose()?;
    let kind = kind
        .map(|text| {
            KINDS
                .iter()
                .find(|(name, _)| *name == text)
                .map(|(_, kind)| *kind)
                .ok_or_else(|| {
                    CliError::Usage(format!(
                        "--kind `{text}` is not a media job kind (stills, poster, transcode)"
                    ))
                })
        })
        .transpose()?;
    Ok(RequeueFilter { account, kind })
}

/// Every lane and status, `<lane>_<status>`, zero when the owner reports none: the same facts in text and JSON.
fn depths(stats: MediaStats) -> Outcome {
    let mut outcome = Outcome::success();
    for (lane_name, lane) in LANES {
        for status in STATUSES {
            let count: u64 = stats
                .counts
                .iter()
                .filter(|item| item.lane == lane && item.status == status)
                .map(|item| item.count)
                .sum();
            outcome = outcome.field(&format!("{}_{}", lane_name, status_name(status)), count);
        }
    }
    outcome
}

fn status_name(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Queued => "queued",
        JobStatus::Running => "running",
        JobStatus::Succeeded => "succeeded",
        JobStatus::Dead => "dead",
    }
}

fn failure(error: MediaAdminError) -> CliError {
    CliError::Operator {
        code: ErrorCode::Internal.as_str(),
        detail: format!("media queue: {error}"),
        exit: Exit::Failed,
    }
}
