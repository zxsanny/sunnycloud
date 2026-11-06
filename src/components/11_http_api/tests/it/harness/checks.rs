use std::sync::{Arc, Mutex};

use sc_observability::{CheckFuture, ReadinessCheck, ReadinessItem, ReadinessStatus};

/// A readiness check whose answer the test flips, standing for the checks 04, 12 and others register.
pub struct SwitchCheck {
    name: &'static str,
    item: Mutex<ReadinessItem>,
}

impl SwitchCheck {
    pub fn ok(name: &'static str) -> Arc<SwitchCheck> {
        Arc::new(SwitchCheck {
            name,
            item: Mutex::new(ReadinessItem::ok()),
        })
    }

    pub fn set(&self, status: ReadinessStatus, detail: &str) {
        *self.item.lock().expect("check state") = ReadinessItem::new(status, detail);
    }
}

impl ReadinessCheck for SwitchCheck {
    fn name(&self) -> &str {
        self.name
    }

    fn check(&self) -> CheckFuture<'_, ReadinessItem> {
        let item = self.item.lock().expect("check state").clone();
        Box::pin(async move { item })
    }
}

/// A check that never answers.
pub struct HungCheck;

impl ReadinessCheck for HungCheck {
    fn name(&self) -> &str {
        "scheduler"
    }

    fn check(&self) -> CheckFuture<'_, ReadinessItem> {
        Box::pin(std::future::pending())
    }
}
