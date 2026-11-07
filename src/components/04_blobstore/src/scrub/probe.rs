//! Test-only seam of the scrubber: a hook awaited at two points of each blob, so a test can let the collector or a
//! reinstate act at exactly that moment. Without the `sync-hook` feature the call is empty.

#[cfg(feature = "sync-hook")]
pub(super) use hooked::Probe;
#[cfg(feature = "sync-hook")]
pub use hooked::{ScrubHook, ScrubPoint};
#[cfg(not(feature = "sync-hook"))]
pub(super) use idle::{Probe, ScrubPoint};

#[cfg(not(feature = "sync-hook"))]
mod idle {
    use uuid::Uuid;

    pub struct Probe;

    #[expect(
        dead_code,
        reason = "the id is read only by the hook of the sync-hook build"
    )]
    pub enum ScrubPoint {
        Opening(Uuid),
        Verified(Uuid),
    }

    impl Probe {
        pub(in crate::scrub) fn new() -> Self {
            Probe
        }

        pub(in crate::scrub) async fn at(&self, _point: ScrubPoint) {}
    }
}

#[cfg(feature = "sync-hook")]
mod hooked {
    use std::sync::Arc;

    use contracts::async_trait;
    use uuid::Uuid;

    /// Where in the check of one blob the hook is awaited.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ScrubPoint {
        /// The blob was selected; its file has not been opened.
        Opening(Uuid),
        /// The whole file was read and judged; a finding has not been re-checked against the row.
        Verified(Uuid),
    }

    #[async_trait]
    pub trait ScrubHook: Send + Sync {
        async fn at(&self, point: ScrubPoint);
    }

    pub struct Probe {
        pub(in crate::scrub) hook: Option<Arc<dyn ScrubHook>>,
    }

    impl Probe {
        pub(in crate::scrub) fn new() -> Self {
            Probe { hook: None }
        }

        pub(in crate::scrub) async fn at(&self, point: ScrubPoint) {
            if let Some(hook) = &self.hook {
                hook.at(point).await;
            }
        }
    }
}
