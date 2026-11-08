use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sc_observability::{CheckFuture, ReadinessCheck, ReadinessItem};

/// Whether the API still accepts the worker secret. The first 403 on any call clears it for every slot and
/// the status reporter: slots stop claiming and `/readyz` answers 503. Only a restart sets it again.
#[derive(Debug, Clone, Default)]
pub struct ApiAccess {
    refused: Arc<AtomicBool>,
}

impl ApiAccess {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_refused(&self) -> bool {
        self.refused.load(Ordering::Acquire)
    }

    pub(crate) fn refuse(&self) {
        self.refused.store(true, Ordering::Release);
    }
}

impl ReadinessCheck for ApiAccess {
    fn name(&self) -> &str {
        "worker_api"
    }

    fn check(&self) -> CheckFuture<'_, ReadinessItem> {
        let item = if self.is_refused() {
            ReadinessItem::failing("the API refused the worker secret")
        } else {
            ReadinessItem::ok()
        };
        Box::pin(std::future::ready(item))
    }
}
