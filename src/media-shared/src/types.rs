//! Job DTOs of the worker listener (`/api/v1/worker-jobs/**`, 08 section 5). The serialised
//! names are the ones the routes use.

use sc_ids::FileHash;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Claim lane. `stills` jobs run in `image`, `poster` and `transcode` jobs in `video`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    Image,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Stills,
    Poster,
    Transcode,
}

/// 200 body of `POST /api/v1/worker-jobs/claims`. `content_hash` is 64 lowercase hex digits,
/// the `Display` form of `FileHash`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimedJob {
    pub job_id: Uuid,
    pub kind: JobKind,
    #[serde(with = "file_hash_hex")]
    pub content_hash: FileHash,
    pub params_version: u32,
    pub attempt: u32,
    pub lease_ms: u64,
}

/// 200 body of `PUT /api/v1/worker-jobs/{job_id}/lease`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseExtended {
    pub lease_ms: u64,
}

/// The `{kind}` path segment of `PUT /api/v1/worker-jobs/{job_id}/outputs/{kind}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedKind {
    Thumb,
    Preview,
    Transcode,
}

/// Output metadata carried by `Content-Type`, `X-Width`, `X-Height` and `X-Duration-Ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputMeta {
    pub media_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
}

/// Outcome of `PUT /api/v1/worker-jobs/{job_id}/result`: the job closes without output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipOutcome {
    SkippedCompliant,
    Obsolete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Retry,
    Permanent,
}

/// Body of `POST /api/v1/worker-jobs/{job_id}/failure`; `code` is a short code, never child output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureReport {
    pub code: String,
    pub disposition: Disposition,
}

/// What the API did with a failure report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailResult {
    Requeued,
    Dead,
}

/// Body of `PUT /api/v1/worker-jobs/status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerStatus {
    pub decoder_ok: bool,
    pub image_age_days: u32,
}

mod file_hash_hex {
    use sc_ids::FileHash;
    use serde::{Deserialize, Deserializer, Serializer, de};

    const HASH_LEN: usize = 32;

    pub fn serialize<S: Serializer>(hash: &FileHash, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(hash)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<FileHash, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse(&text)
            .map(FileHash::from_bytes)
            .ok_or_else(|| de::Error::custom("content_hash must be 64 lowercase hex digits"))
    }

    fn parse(text: &str) -> Option<[u8; HASH_LEN]> {
        let digits = text.as_bytes();
        if digits.len() != HASH_LEN * 2 {
            return None;
        }
        let mut bytes = [0u8; HASH_LEN];
        for (byte, pair) in bytes.iter_mut().zip(digits.chunks_exact(2)) {
            *byte = (nibble(pair[0])? << 4) | nibble(pair[1])?;
        }
        Some(bytes)
    }

    fn nibble(digit: u8) -> Option<u8> {
        match digit {
            b'0'..=b'9' => Some(digit - b'0'),
            b'a'..=b'f' => Some(digit - b'a' + 10),
            _ => None,
        }
    }
}
