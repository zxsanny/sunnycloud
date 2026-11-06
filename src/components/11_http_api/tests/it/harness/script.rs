use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use contracts::Fault;

/// What an unscripted port call fails with: never a default success.
#[derive(Debug, thiserror::Error)]
#[error("unscripted port call {0}")]
pub struct UnscriptedCall(pub &'static str);

/// The results the test stubs hand out, keyed `"Port::method"`. A key holds a queue; each call takes the
/// front. A call with nothing queued fails as `Unexpected(UnscriptedCall)`.
#[derive(Clone, Default)]
pub struct Script {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    results: HashMap<&'static str, VecDeque<Box<dyn Any + Send>>>,
    calls: HashMap<&'static str, usize>,
}

impl Script {
    /// Queues `result` (the full `Result` the port method returns) for the next call of `key`.
    pub fn push<T: Send + 'static>(&self, key: &'static str, result: T) {
        self.lock()
            .results
            .entry(key)
            .or_default()
            .push_back(Box::new(result));
    }

    /// How many times `key` was called, scripted or not.
    pub fn calls(&self, key: &'static str) -> usize {
        self.lock().calls.get(key).copied().unwrap_or(0)
    }

    pub(super) fn take<T: 'static>(&self, key: &'static str) -> Option<T> {
        let mut inner = self.lock();
        *inner.calls.entry(key).or_default() += 1;
        let boxed = inner.results.get_mut(key)?.pop_front()?;
        Some(
            *boxed
                .downcast::<T>()
                .unwrap_or_else(|_| panic!("the value scripted for {key} has the wrong type")),
        )
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub(super) fn unscripted(key: &'static str) -> Fault {
    Box::new(UnscriptedCall(key))
}
