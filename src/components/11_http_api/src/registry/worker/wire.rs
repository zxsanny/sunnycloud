//! The JSON of the worker routes. The names are those of `media-shared` (08 section 5), which 11 may not import
//! (`part3_blob_share_media_vault.md`, open point 2); the wire test in E-18/E-20 keeps the two equal.

use contracts::{
    ClaimedJob, FailDisposition, FailResult, JobKind, Lane, SkipOutcome, WorkerStatus,
};
use serde_json::{Value, json};

use crate::json_body::fields;
use crate::registry::SchemaRef;

pub(super) fn claim_lane(body: &Value) -> Option<Lane> {
    let [lane] = fields(body, ["lane"])?;
    match lane.as_str()? {
        "image" => Some(Lane::Image),
        "video" => Some(Lane::Video),
        _ => None,
    }
}

pub(super) fn skip_outcome(body: &Value) -> Option<SkipOutcome> {
    let [outcome] = fields(body, ["outcome"])?;
    match outcome.as_str()? {
        "skipped_compliant" => Some(SkipOutcome::SkippedCompliant),
        "obsolete" => Some(SkipOutcome::Obsolete),
        _ => None,
    }
}

pub(super) fn failure(body: &Value) -> Option<(&str, FailDisposition)> {
    let [code, disposition] = fields(body, ["code", "disposition"])?;
    let disposition = match disposition.as_str()? {
        "retry" => FailDisposition::Retry,
        "permanent" => FailDisposition::Permanent,
        _ => return None,
    };
    Some((code.as_str()?, disposition))
}

pub(super) fn worker_status(body: &Value) -> Option<WorkerStatus> {
    let [decoder_ok, image_age_days] = fields(body, ["decoder_ok", "image_age_days"])?;
    Some(WorkerStatus {
        decoder_ok: decoder_ok.as_bool()?,
        image_age_days: u32::try_from(image_age_days.as_u64()?).ok()?,
    })
}

pub(super) fn claimed_job(job: &ClaimedJob) -> Value {
    let kind = match job.kind {
        JobKind::Stills => "stills",
        JobKind::Poster => "poster",
        JobKind::Transcode => "transcode",
    };
    json!({
        "job_id": job.job_id,
        "kind": kind,
        "content_hash": job.content_hash.to_string(),
        "params_version": job.params_version,
        "attempt": job.attempt,
        "lease_ms": job.lease_ms,
    })
}

pub(super) fn lease_extended(lease_ms: u64) -> Value {
    json!({"lease_ms": lease_ms})
}

pub(super) fn fail_result(result: FailResult) -> Value {
    let result = match result {
        FailResult::Requeued => "requeued",
        FailResult::Dead => "dead",
    };
    json!({"result": result})
}

pub(super) const CLAIM_REQUEST: SchemaRef = SchemaRef {
    name: "WorkerClaimRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["lane"],
            "additionalProperties": false,
            "properties": {"lane": {"type": "string", "enum": ["image", "video"]}}
        })
    },
};

pub(super) const CLAIMED_JOB: SchemaRef = SchemaRef {
    name: "ClaimedJob",
    build: || {
        json!({
            "type": "object",
            "required": ["job_id", "kind", "content_hash", "params_version", "attempt", "lease_ms"],
            "properties": {
                "job_id": {"type": "string", "format": "uuid"},
                "kind": {"type": "string", "enum": ["stills", "poster", "transcode"]},
                "content_hash": {
                    "type": "string",
                    "description": "64 lowercase hex digits."
                },
                "params_version": {"type": "integer", "minimum": 0},
                "attempt": {"type": "integer", "minimum": 0},
                "lease_ms": {"type": "integer", "minimum": 0}
            }
        })
    },
};

pub(super) const LEASE_EXTENDED: SchemaRef = SchemaRef {
    name: "LeaseExtended",
    build: || {
        json!({
            "type": "object",
            "required": ["lease_ms"],
            "properties": {"lease_ms": {"type": "integer", "minimum": 0}}
        })
    },
};

pub(super) const RESULT_REQUEST: SchemaRef = SchemaRef {
    name: "WorkerResultRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["outcome"],
            "additionalProperties": false,
            "properties": {"outcome": {"type": "string", "enum": ["skipped_compliant", "obsolete"]}}
        })
    },
};

pub(super) const FAILURE_REQUEST: SchemaRef = SchemaRef {
    name: "WorkerFailureRequest",
    build: || {
        json!({
            "type": "object",
            "required": ["code", "disposition"],
            "additionalProperties": false,
            "properties": {
                "code": {"type": "string"},
                "disposition": {"type": "string", "enum": ["retry", "permanent"]}
            }
        })
    },
};

pub(super) const FAILURE_RESULT: SchemaRef = SchemaRef {
    name: "WorkerFailureResult",
    build: || {
        json!({
            "type": "object",
            "required": ["result"],
            "properties": {"result": {"type": "string", "enum": ["requeued", "dead"]}}
        })
    },
};

pub(super) const STATUS_REQUEST: SchemaRef = SchemaRef {
    name: "WorkerStatus",
    build: || {
        json!({
            "type": "object",
            "required": ["decoder_ok", "image_age_days"],
            "additionalProperties": false,
            "properties": {
                "decoder_ok": {"type": "boolean"},
                "image_age_days": {"type": "integer", "minimum": 0}
            }
        })
    },
};
