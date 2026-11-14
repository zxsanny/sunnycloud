//! The text forms of the job enums as the `jobs` columns hold them.

use media_shared::types::{JobKind, Lane, SkipOutcome};
use sc_db::DbError;

pub(crate) fn kind_name(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Stills => "stills",
        JobKind::Poster => "poster",
        JobKind::Transcode => "transcode",
    }
}

pub(crate) fn lane_name(lane: Lane) -> &'static str {
    match lane {
        Lane::Image => "image",
        Lane::Video => "video",
    }
}

pub(crate) fn parse_kind(name: &str) -> Result<JobKind, DbError> {
    match name {
        "stills" => Ok(JobKind::Stills),
        "poster" => Ok(JobKind::Poster),
        "transcode" => Ok(JobKind::Transcode),
        other => Err(DbError::Other(format!("unknown job kind {other:?}"))),
    }
}

pub(crate) fn skip_name(outcome: SkipOutcome) -> &'static str {
    match outcome {
        SkipOutcome::SkippedCompliant => "skipped_compliant",
        SkipOutcome::Obsolete => "obsolete",
    }
}
