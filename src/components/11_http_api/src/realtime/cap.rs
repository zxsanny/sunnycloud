//! The per-account cap on open realtime sockets (`realtime.md`, Endpoint). The same shape as 05's `HoldCap`; 11
//! may not depend on a component crate (crate-graph rule), so it keeps its own 20 lines.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use uuid::Uuid;

#[derive(Debug)]
pub(super) struct ConnectionCap {
    limit: usize,
    open: Mutex<HashMap<Uuid, usize>>,
}

/// One open socket's slot; dropping it, however the socket ends, frees the slot.
#[derive(Debug)]
pub(super) struct ConnectionPermit {
    cap: Arc<ConnectionCap>,
    account: Uuid,
}

impl ConnectionCap {
    pub(super) fn new(limit: usize) -> Arc<Self> {
        Arc::new(ConnectionCap {
            limit,
            open: Mutex::default(),
        })
    }

    /// `None` when the account already has `limit` sockets open.
    pub(super) fn acquire(self: &Arc<Self>, account: Uuid) -> Option<ConnectionPermit> {
        let mut open = self.open();
        let count = open.entry(account).or_insert(0);
        if *count >= self.limit {
            return None;
        }
        *count += 1;
        Some(ConnectionPermit {
            cap: Arc::clone(self),
            account,
        })
    }

    fn open(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, usize>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        let mut open = self.cap.open();
        if let Some(count) = open.get_mut(&self.account) {
            *count -= 1;
            if *count == 0 {
                open.remove(&self.account);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: Uuid = Uuid::from_u128(1);
    const OTHER: Uuid = Uuid::from_u128(2);

    #[test]
    fn the_account_at_its_limit_gets_no_slot_and_a_freed_slot_is_reusable() {
        // Arrange
        let cap = ConnectionCap::new(2);
        let first = cap.acquire(ACCOUNT);
        let second = cap.acquire(ACCOUNT);

        // Act
        let third = cap.acquire(ACCOUNT);
        drop(first);
        let after_free = cap.acquire(ACCOUNT);

        // Assert
        assert!(second.is_some() && third.is_none() && after_free.is_some());
    }

    #[test]
    fn another_account_is_counted_apart_and_no_entry_outlives_its_last_slot() {
        // Arrange
        let cap = ConnectionCap::new(1);
        let held = cap.acquire(ACCOUNT);

        // Act
        let other = cap.acquire(OTHER);
        drop((held, other));

        // Assert
        assert!(cap.open().is_empty());
    }
}
