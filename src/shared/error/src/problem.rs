//! The problem body schema (RFC 9457 `application/problem+json`). Field order is fixed so equal inputs give equal bytes.

use crate::business::{BusinessParams, FieldProblem, ResyncReason};
use crate::code::ErrorCode;
use crate::correlation::CorrelationId;
use serde::{Serialize, Serializer};

/// Everything the wire body is built from. Serialising it is the only way a body is produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProblemBody {
    pub code: ErrorCode,
    pub request_id: CorrelationId,
    pub params: BusinessParams,
    pub problems: Vec<FieldProblem>,
}

impl ProblemBody {
    /// A body with no parameters and no field problems.
    pub fn plain(code: ErrorCode, request_id: CorrelationId) -> ProblemBody {
        ProblemBody {
            code,
            request_id,
            params: BusinessParams::default(),
            problems: Vec::new(),
        }
    }
}

#[derive(Serialize)]
struct Wire<'a> {
    #[serde(rename = "type")]
    type_uri: String,
    title: &'static str,
    status: u16,
    code: &'static str,
    request_id: &'a CorrelationId,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after: Option<u64>,
    #[serde(skip_serializing_if = "slice_is_empty")]
    need_blocks: &'a [u32],
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<ResyncReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_version: Option<&'a str>,
    #[serde(skip_serializing_if = "slice_is_empty")]
    problems: &'a [FieldProblem],
}

fn slice_is_empty<T>(slice: &&[T]) -> bool {
    slice.is_empty()
}

impl Serialize for ProblemBody {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let retry_after = self.params.retry_after.map(|delay| {
            delay
                .as_secs()
                .saturating_add(u64::from(delay.subsec_nanos() > 0))
        });
        Wire {
            type_uri: format!("urn:sunnycloud:error:{}", self.code.as_str()),
            title: self.code.title(),
            status: self.code.status(),
            code: self.code.as_str(),
            request_id: &self.request_id,
            retry_after,
            need_blocks: &self.params.need_blocks,
            reason: self.params.reason,
            min_version: self.params.min_version.as_deref(),
            problems: &self.problems,
        }
        .serialize(serializer)
    }
}
