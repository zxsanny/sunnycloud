use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use uuid::Uuid;

/// Concurrent holds per principal (session or device id). An entry exists only while the principal holds a
/// read, so the map is bounded by the number of live holds.
#[derive(Debug)]
pub struct HoldCap {
    limit: u64,
    held: Mutex<HashMap<Uuid, u64>>,
}

/// One hold's slot. Dropping it, on completion, cancellation or panic, frees the slot.
#[derive(Debug)]
pub struct HoldPermit {
    cap: Arc<HoldCap>,
    principal: Uuid,
}

impl HoldCap {
    pub fn new(limit: u64) -> Arc<Self> {
        Arc::new(HoldCap {
            limit,
            held: Mutex::default(),
        })
    }

    /// `None` when the principal already holds `limit` reads.
    pub fn acquire(self: &Arc<Self>, principal: Uuid) -> Option<HoldPermit> {
        let mut held = self.held();
        let count = held.entry(principal).or_insert(0);
        if *count >= self.limit {
            return None;
        }
        *count += 1;
        Some(HoldPermit {
            cap: self.clone(),
            principal,
        })
    }

    pub fn holding(&self, principal: Uuid) -> u64 {
        self.held().get(&principal).copied().unwrap_or(0)
    }

    pub fn principals(&self) -> usize {
        self.held().len()
    }

    fn held(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, u64>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for HoldPermit {
    fn drop(&mut self) {
        let mut held = self.cap.held();
        if let Some(count) = held.get_mut(&self.principal) {
            *count -= 1;
            if *count == 0 {
                held.remove(&self.principal);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    const ME: Uuid = Uuid::from_u128(1);
    const OTHER: Uuid = Uuid::from_u128(2);

    #[test]
    fn the_fifth_hold_is_refused_and_a_released_slot_is_reusable() {
        // Arrange
        let cap = HoldCap::new(4);
        let mut permits: Vec<_> = (0..4).map(|_| cap.acquire(ME).expect("slot")).collect();

        // Act
        let fifth = cap.acquire(ME);
        let other = cap.acquire(OTHER);
        permits.pop();
        let after_release = cap.acquire(ME);

        // Assert
        assert!(fifth.is_none());
        assert!(other.is_some());
        assert!(after_release.is_some());
        assert_eq!(cap.holding(ME), 4);
    }

    #[test]
    fn the_entry_is_removed_when_the_last_permit_is_dropped() {
        // Arrange
        let cap = HoldCap::new(4);
        let permit = cap.acquire(ME).expect("slot");

        // Act
        drop(permit);

        // Assert
        assert_eq!(cap.principals(), 0);
    }

    #[test]
    fn a_panic_while_holding_releases_the_slot() {
        // Arrange
        let cap = HoldCap::new(1);

        // Act
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let _permit = cap.acquire(ME).expect("slot");
            panic!("hold panicked");
        }));

        // Assert
        assert!(outcome.is_err());
        assert!(cap.acquire(ME).is_some());
    }
}
