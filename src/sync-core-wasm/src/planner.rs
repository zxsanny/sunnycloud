//! `UploadPlanner` for the Worker: the `sync-core-proto` state machine behind a wasm-bindgen class.
//! Actions leave and results enter as tagged objects `{kind, ...}`.

use std::time::Duration;

use sync_core_proto::{Action, BlockResult, CommitResult, RetryPolicy};
use wasm_bindgen::prelude::*;

use crate::ExportError;

/// One planner action: `kind` is the `sync-core-proto` variant name; `i` is set for `SendBlock` and
/// `GiveUp`, `ms` for `Wait`.
#[wasm_bindgen(js_name = Action, inspectable)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JsAction {
    kind: &'static str,
    i: Option<u32>,
    ms: Option<f64>,
}

#[wasm_bindgen(js_class = Action)]
impl JsAction {
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        self.kind.to_string()
    }

    #[wasm_bindgen(getter)]
    pub fn i(&self) -> Option<u32> {
        self.i
    }

    #[wasm_bindgen(getter)]
    pub fn ms(&self) -> Option<f64> {
        self.ms
    }
}

impl From<Action> for JsAction {
    fn from(action: Action) -> Self {
        let (kind, i, ms) = match action {
            Action::SendBlock { i } => ("SendBlock", Some(i), None),
            // Exact below 2^53 ms; delays come from u32 ms inputs, far below that.
            Action::Wait { ms } => ("Wait", None, Some(ms as f64)),
            Action::RereadSession => ("RereadSession", None, None),
            Action::Commit => ("Commit", None, None),
            Action::Done => ("Done", None, None),
            Action::RestartSession => ("RestartSession", None, None),
            Action::GiveUp { i } => ("GiveUp", Some(i), None),
        };
        Self { kind, i, ms }
    }
}

#[wasm_bindgen(typescript_custom_section)]
const RESULT_TYPES: &str = r#"
export type BlockResult =
  | { kind: "Acked" | "HashRejected" | "SessionGone" }
  | { kind: "Transient" | "BlockConflict"; retry_after_ms?: number };
export type CommitResult =
  | { kind: "NeedBlocks"; list: Uint32Array | number[] }
  | { kind: "BlocklistMismatch" | "Ok" | "SessionGone" };
"#;

#[wasm_bindgen]
extern "C" {
    /// `{kind: "Acked" | "HashRejected" | "Transient" | "BlockConflict" | "SessionGone", retry_after_ms?}`
    #[wasm_bindgen(typescript_type = "BlockResult")]
    pub type BlockResultValue;
    #[wasm_bindgen(method, getter)]
    fn kind(this: &BlockResultValue) -> Option<String>;
    #[wasm_bindgen(method, getter)]
    fn retry_after_ms(this: &BlockResultValue) -> Option<f64>;

    /// `{kind: "NeedBlocks" | "BlocklistMismatch" | "Ok" | "SessionGone", list?}`; `list` for `NeedBlocks`.
    #[wasm_bindgen(typescript_type = "CommitResult")]
    pub type CommitResultValue;
    #[wasm_bindgen(method, getter)]
    fn kind(this: &CommitResultValue) -> Option<String>;
    #[wasm_bindgen(method, getter)]
    fn list(this: &CommitResultValue) -> Option<Vec<u32>>;
}

/// A block result from its tagged-object fields.
pub fn block_result(kind: &str, retry_after_ms: Option<f64>) -> Result<BlockResult, ExportError> {
    let retry_after = retry_after_ms.map(duration_ms).transpose()?;
    match kind {
        "Acked" => Ok(BlockResult::Acked),
        "HashRejected" => Ok(BlockResult::HashRejected),
        "Transient" => Ok(BlockResult::Transient { retry_after }),
        "BlockConflict" => Ok(BlockResult::BlockConflict { retry_after }),
        "SessionGone" => Ok(BlockResult::SessionGone),
        _ => Err(ExportError::InvalidResult),
    }
}

/// A commit result from its tagged-object fields.
pub fn commit_result(kind: &str, list: Option<Vec<u32>>) -> Result<CommitResult, ExportError> {
    match kind {
        "NeedBlocks" => list
            .map(|list| CommitResult::NeedBlocks { list })
            .ok_or(ExportError::InvalidResult),
        "BlocklistMismatch" => Ok(CommitResult::BlocklistMismatch),
        "Ok" => Ok(CommitResult::Ok),
        "SessionGone" => Ok(CommitResult::SessionGone),
        _ => Err(ExportError::InvalidResult),
    }
}

fn duration_ms(ms: f64) -> Result<Duration, ExportError> {
    if !ms.is_finite() || ms < 0.0 || ms.fract() != 0.0 || ms > u32::MAX as f64 {
        return Err(ExportError::InvalidResult);
    }
    Ok(Duration::from_millis(ms as u64))
}

/// The upload planner of `sync-core-proto` (`sync_core_proto.md`), one instance per upload session.
#[wasm_bindgen]
pub struct UploadPlanner {
    inner: sync_core_proto::UploadPlanner,
}

#[wasm_bindgen]
impl UploadPlanner {
    #[wasm_bindgen(constructor)]
    pub fn new(
        block_count: u32,
        need_blocks: &[u32],
        lanes_max: u32,
        retry_base_ms: u32,
        retry_max_ms: u32,
        max_attempts: u32,
    ) -> UploadPlanner {
        let policy = RetryPolicy {
            base: Duration::from_millis(u64::from(retry_base_ms)),
            max: Duration::from_millis(u64::from(retry_max_ms)),
            max_attempts_per_block: max_attempts,
        };
        UploadPlanner {
            inner: sync_core_proto::UploadPlanner::new(block_count, need_blocks, lanes_max, policy),
        }
    }

    pub fn next_actions(&mut self) -> Vec<JsAction> {
        self.inner
            .next_actions()
            .into_iter()
            .map(JsAction::from)
            .collect()
    }

    /// Throws `InvalidResult` for an unknown `kind` or a bad `retry_after_ms`; the state is unchanged then.
    pub fn on_result(&mut self, i: u32, result: &BlockResultValue) -> Result<(), JsValue> {
        let kind = result.kind().ok_or(ExportError::InvalidResult)?;
        let result = block_result(&kind, result.retry_after_ms())?;
        self.inner.on_result(i, result);
        Ok(())
    }

    pub fn on_session(&mut self, need_blocks: &[u32]) {
        self.inner.on_session(need_blocks);
    }

    /// Throws `InvalidResult` for an unknown `kind` or `NeedBlocks` without `list`.
    pub fn on_commit(&mut self, result: &CommitResultValue) -> Result<(), JsValue> {
        let kind = result.kind().ok_or(ExportError::InvalidResult)?;
        let result = commit_result(&kind, result.list())?;
        self.inner.on_commit(result);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_results_map_to_proto_variants() {
        // Arrange
        let cases = [
            ("Acked", None, BlockResult::Acked),
            ("HashRejected", None, BlockResult::HashRejected),
            (
                "Transient",
                Some(500.0),
                BlockResult::Transient {
                    retry_after: Some(Duration::from_millis(500)),
                },
            ),
            (
                "BlockConflict",
                None,
                BlockResult::BlockConflict { retry_after: None },
            ),
            ("SessionGone", None, BlockResult::SessionGone),
        ];

        // Act
        let mapped: Vec<_> = cases
            .iter()
            .map(|(kind, ms, _)| block_result(kind, *ms))
            .collect();

        // Assert
        let expected: Vec<_> = cases.iter().map(|(_, _, want)| Ok(*want)).collect();
        assert_eq!(mapped, expected);
    }

    #[test]
    fn malformed_results_are_refused() {
        // Arrange
        let bad_kind = ("Lost", None);
        let bad_delay = ("Transient", Some(-1.0));

        // Act
        let kind_result = block_result(bad_kind.0, bad_kind.1);
        let delay_result = block_result(bad_delay.0, bad_delay.1);
        let need_without_list = commit_result("NeedBlocks", None);

        // Assert
        assert_eq!(kind_result, Err(ExportError::InvalidResult));
        assert_eq!(delay_result, Err(ExportError::InvalidResult));
        assert_eq!(need_without_list, Err(ExportError::InvalidResult));
    }

    #[test]
    fn actions_become_tagged_values() {
        // Arrange
        let actions = [
            Action::SendBlock { i: 3 },
            Action::Wait { ms: 250 },
            Action::Done,
        ];

        // Act
        let tagged: Vec<JsAction> = actions.into_iter().map(JsAction::from).collect();

        // Assert
        assert_eq!(
            tagged,
            vec![
                JsAction {
                    kind: "SendBlock",
                    i: Some(3),
                    ms: None
                },
                JsAction {
                    kind: "Wait",
                    i: None,
                    ms: Some(250.0)
                },
                JsAction {
                    kind: "Done",
                    i: None,
                    ms: None
                },
            ]
        );
    }
}
