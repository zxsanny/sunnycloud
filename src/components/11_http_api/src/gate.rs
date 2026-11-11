use std::sync::Arc;

use sc_observability::{NoLabels, log_warn};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::metrics::{ConnectionMetrics, RejectReason};

/// The global connection cap shared by every listener of one server.
pub(crate) struct ConnectionGate {
    permits: Arc<Semaphore>,
    capacity: u32,
    metrics: ConnectionMetrics,
}

/// Held for the life of a connection task; dropping it (also on a panic) frees the slot.
pub(crate) struct ConnectionGuard {
    _permit: OwnedSemaphorePermit,
    gate: Arc<ConnectionGate>,
}

impl ConnectionGate {
    pub(crate) fn new(max_connections: u32, metrics: ConnectionMetrics) -> Arc<Self> {
        Arc::new(ConnectionGate {
            permits: Arc::new(Semaphore::new(max_connections as usize)),
            capacity: max_connections,
            metrics,
        })
    }

    /// Returns when no connection is open: every permit is back. While it waits no new admission succeeds.
    pub(crate) async fn wait_idle(&self) {
        // The semaphore is never closed, so an error here is a bug worth a line in the log.
        if let Err(error) = self.permits.acquire_many(self.capacity).await {
            log_warn!(
                "connection gate closed while waiting for connections",
                err = &error
            );
        }
    }

    /// `None` when the cap is reached; the refusal is counted.
    pub(crate) fn admit(self: &Arc<Self>) -> Option<ConnectionGuard> {
        // The semaphore is never closed, so the only error is `NoPermits`.
        match Arc::clone(&self.permits).try_acquire_owned() {
            Ok(permit) => {
                self.metrics.open.add(NoLabels, 1.0);
                Some(ConnectionGuard {
                    _permit: permit,
                    gate: Arc::clone(self),
                })
            }
            Err(_) => {
                self.metrics.rejected.inc(RejectReason::Limit);
                None
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn rejected(&self) -> u64 {
        self.metrics.rejected.get(RejectReason::Limit)
    }

    #[cfg(test)]
    pub(crate) fn open(&self) -> f64 {
        self.metrics.open.get(NoLabels)
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.gate.metrics.open.add(NoLabels, -1.0);
    }
}

#[cfg(test)]
mod tests {
    use sc_observability::{BuildInfo, Metrics};

    use super::*;

    fn gate(max: u32) -> Arc<ConnectionGate> {
        let build = BuildInfo {
            version: "test",
            commit: "test",
            test_clock: false,
        };
        let metrics = Metrics::new(build).expect("registry");
        let connection_metrics = ConnectionMetrics::register(&metrics).expect("families");
        ConnectionGate::new(max, connection_metrics)
    }

    #[test]
    fn sixty_admissions_against_fifty_permits_refuse_and_count_ten() {
        // Arrange
        let gate = gate(50);

        // Act
        let guards: Vec<_> = (0..60).filter_map(|_| gate.admit()).collect();

        // Assert
        assert_eq!(guards.len(), 50);
        assert_eq!(gate.rejected(), 10);
        assert_eq!(gate.open(), 50.0);
    }

    #[tokio::test]
    async fn wait_idle_returns_once_the_last_guard_is_dropped() {
        // Arrange
        let gate = gate(3);
        let guards: Vec<_> = (0..3).filter_map(|_| gate.admit()).collect();
        let waiter = tokio::spawn({
            let gate = Arc::clone(&gate);
            async move { gate.wait_idle().await }
        });
        tokio::task::yield_now().await;

        // Act
        let still_waiting = !waiter.is_finished();
        drop(guards);
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), waiter).await;

        // Assert
        assert!(still_waiting, "three connections are still open");
        assert!(outcome.is_ok(), "the wait ends when they are all gone");
    }

    #[test]
    fn dropping_a_guard_frees_its_slot_and_lowers_the_open_gauge() {
        // Arrange
        let gate = gate(1);
        let held = gate.admit().expect("first admission");
        assert!(gate.admit().is_none());

        // Act
        drop(held);

        // Assert
        assert_eq!(gate.open(), 0.0);
        assert!(gate.admit().is_some());
        assert_eq!(gate.rejected(), 1);
    }
}
