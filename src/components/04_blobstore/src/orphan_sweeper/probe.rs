//! Test-only seam of the sweeper: a hook awaited after each directory listing and before each re-check, so a test
//! can change the database or the volume at exactly that moment. Without the `sync-hook` feature the call is empty.

#[cfg(feature = "sync-hook")]
pub use hooked::{OrphanHook, OrphanPoint, Probe};
#[cfg(not(feature = "sync-hook"))]
pub(super) use idle::{OrphanPoint, Probe};

#[cfg(not(feature = "sync-hook"))]
mod idle {
    use uuid::Uuid;

    pub struct Probe;

    #[expect(
        dead_code,
        reason = "the fields are read only by the hook of the sync-hook build"
    )]
    pub enum OrphanPoint {
        Listed(u16),
        BeforeRecheck(Uuid),
    }

    impl Probe {
        pub(in crate::orphan_sweeper) fn new() -> Self {
            Probe
        }

        pub(in crate::orphan_sweeper) async fn at(&self, _point: OrphanPoint) {}
    }
}

#[cfg(feature = "sync-hook")]
mod hooked {
    use std::sync::Arc;

    use contracts::async_trait;
    use uuid::Uuid;

    /// Where in a run the hook is awaited.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum OrphanPoint {
        /// The prefix directory (`aabb` as a number) has been listed; its rows have not been queried.
        Listed(u16),
        /// The scan is complete; the row of this orphan candidate has not been re-checked.
        BeforeRecheck(Uuid),
    }

    #[async_trait]
    pub trait OrphanHook: Send + Sync {
        async fn at(&self, point: OrphanPoint);
    }

    pub struct Probe {
        pub(in crate::orphan_sweeper) hook: Option<Arc<dyn OrphanHook>>,
    }

    impl Probe {
        pub(in crate::orphan_sweeper) fn new() -> Self {
            Probe { hook: None }
        }

        pub(in crate::orphan_sweeper) async fn at(&self, point: OrphanPoint) {
            if let Some(hook) = &self.hook {
                hook.at(point).await;
            }
        }
    }
}
