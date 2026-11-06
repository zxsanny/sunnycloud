//! Worker job DTOs, field for field those of `media-shared`. `FailDisposition` is `media-shared`'s
//! `Disposition`; the name differs because `Disposition` here already means inline or attachment (03).

use sc_ids::FileHash;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Image,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Stills,
    Poster,
    Transcode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimedJob {
    pub job_id: Uuid,
    pub kind: JobKind,
    pub content_hash: FileHash,
    pub params_version: u32,
    pub attempt: u32,
    pub lease_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseExtended {
    pub lease_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedKind {
    Thumb,
    Preview,
    Transcode,
}

/// The type and size facts the worker reports about one output; the API sniffs the type itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputMeta {
    pub media_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
}

/// How a job closes without output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipOutcome {
    SkippedCompliant,
    Obsolete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailDisposition {
    Retry,
    Permanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailResult {
    Requeued,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerStatus {
    pub decoder_ok: bool,
    pub image_age_days: u32,
}
