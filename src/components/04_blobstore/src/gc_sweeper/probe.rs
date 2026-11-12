//! Test-only seam of the sweeper: a hook awaited at fixed points of a batch, which can hold it, observe the
//! database or stop the batch as a crash would. Without the `sync-hook` feature the call is empty.

#[cfg(feature = "sync-hook")]
pub use hooked::{GcHook, GcHookStop, GcPoint, Probe};
#[cfg(not(feature = "sync-hook"))]
pub(super) use idle::{GcPoint, Probe};

#[cfg(not(feature = "sync-hook"))]
mod idle {
    use std::convert::Infallible;

    pub struct Probe;

    pub enum GcPoint {
        Claimed,
        Parked,
        BeforeSettle,
    }

    impl Probe {
        pub(in crate::gc_sweeper) fn new() -> Self {
            Probe
        }

        pub(in crate::gc_sweeper) async fn at(&self, _point: GcPoint) -> Result<(), Infallible> {
            Ok(())
        }
    }
}

#[cfg(feature = "sync-hook")]
mod hooked {
    use std::sync::Arc;

    use contracts::async_trait;

    /// Where in a batch the hook is awaited.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum GcPoint {
        /// The claim transaction has committed; no file has been touched.
        Claimed,
        /// One blob's file has been taken out of its path.
        Parked,
        /// Every file of the batch is parked; the settling transaction has not started.
        BeforeSettle,
    }

    /// The hook ends the batch here, as a process death would: nothing after this point of the batch runs.
    #[derive(Debug)]
    pub struct GcHookStop;

    #[async_trait]
    pub trait GcHook: Send + Sync {
        async fn at(&self, point: GcPoint) -> Result<(), GcHookStop>;
    }

    pub struct Probe {
        pub(in crate::gc_sweeper) hook: Option<Arc<dyn GcHook>>,
    }

    impl Probe {
        pub(in crate::gc_sweeper) fn new() -> Self {
            Probe { hook: None }
        }

        pub(in crate::gc_sweeper) async fn at(&self, point: GcPoint) -> Result<(), GcHookStop> {
            match &self.hook {
                Some(hook) => hook.at(point).await,
                None => Ok(()),
            }
        }
    }
}
