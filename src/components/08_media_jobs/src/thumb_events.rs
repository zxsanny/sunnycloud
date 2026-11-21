//! The announcement of a preview that reached a final state (`ThumbNotifier`, `realtime.md`): ready when the
//! job that produces a `thumb` closed with its outputs stored, failed when that job died. Called after the
//! transaction that made it so; nothing here touches the database.

use std::sync::Arc;

use contracts::{ThumbEvent, ThumbNotifier, ThumbState};
use media_shared::types::DerivedKind;

use crate::job_repo::Closed;

pub(crate) fn announce(
    notifier: Option<&Arc<dyn ThumbNotifier>>,
    closed: &Closed,
    state: ThumbState,
) {
    let Some(notifier) = notifier else { return };
    if !closed.kind.outputs().contains(&DerivedKind::Thumb) {
        return;
    }
    notifier.notify(ThumbEvent {
        account: closed.account,
        content_hash: closed.content_hash,
        state,
    });
}
