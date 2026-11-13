//! The in-flight caps shared by every block that is buffered before its write: per account, blocks in flight
//! (large and staged) against `max_inflight_blocks_per_account`; process-wide, buffered bytes against
//! `max_buffered_block_bytes`. A block is admitted before any body byte is read, or refused (`Overloaded`).
//! The per-session lane cap lives with the session's progress (`SessionProgress::start_block`).

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use uuid::Uuid;

#[derive(Debug, Default)]
struct Counts {
    blocks: HashMap<Uuid, u64>,
    bytes: u64,
}

#[derive(Debug)]
pub(crate) struct InflightCaps {
    max_account_blocks: u64,
    max_bytes: u64,
    counts: Mutex<Counts>,
}

/// One admitted block; dropping it gives its block and bytes back.
#[derive(Debug)]
pub(crate) struct Permit<'a> {
    caps: &'a InflightCaps,
    account: Uuid,
    bytes: u64,
}

impl InflightCaps {
    pub(crate) fn new(max_account_blocks: u64, max_bytes: u64) -> Self {
        InflightCaps {
            max_account_blocks,
            max_bytes,
            counts: Mutex::new(Counts::default()),
        }
    }

    /// Admits one block of `bytes` for `account`; `None` when either cap would be exceeded.
    pub(crate) fn admit(&self, account: Uuid, bytes: u64) -> Option<Permit<'_>> {
        let mut counts = self.lock();
        let blocks = counts.blocks.get(&account).copied().unwrap_or(0);
        let buffered = counts.bytes.checked_add(bytes)?;
        if blocks >= self.max_account_blocks || buffered > self.max_bytes {
            return None;
        }
        counts.blocks.insert(account, blocks + 1);
        counts.bytes = buffered;
        Some(Permit {
            caps: self,
            account,
            bytes,
        })
    }

    /// The counters are updated together under the lock and no update can panic midway.
    fn lock(&self) -> MutexGuard<'_, Counts> {
        self.counts.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut counts = self.caps.lock();
        counts.bytes = counts.bytes.saturating_sub(self.bytes);
        if let Some(blocks) = counts.blocks.get_mut(&self.account) {
            *blocks = blocks.saturating_sub(1);
            if *blocks == 0 {
                counts.blocks.remove(&self.account);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: u64 = 2 << 20;

    #[test]
    fn an_account_is_refused_beyond_its_blocks_in_flight_and_others_are_not() {
        // Arrange
        let caps = InflightCaps::new(32, 1 << 30);
        let (busy, other) = (Uuid::new_v4(), Uuid::new_v4());
        let held: Vec<_> = (0..32)
            .map(|_| caps.admit(busy, BLOCK).expect("within the cap"))
            .collect();

        // Act
        let refused = caps.admit(busy, BLOCK);
        let elsewhere = caps.admit(other, BLOCK);

        // Assert
        assert!(refused.is_none());
        assert!(elsewhere.is_some());
        assert_eq!(held.len(), 32);
    }

    #[test]
    fn staged_and_large_blocks_share_the_account_cap() {
        // Arrange: a staged body of 1 KiB and large blocks take the same permits (stage_small admits through
        // this cap as put_block does).
        let caps = InflightCaps::new(2, 1 << 30);
        let account = Uuid::new_v4();
        let staged = caps.admit(account, 1024).expect("staged write");
        let large = caps.admit(account, BLOCK).expect("large block");

        // Act
        let third = caps.admit(account, BLOCK);
        drop(staged);
        let after_release = caps.admit(account, BLOCK);

        // Assert
        assert!(third.is_none());
        assert!(after_release.is_some());
        drop(large);
    }

    #[test]
    fn the_global_buffer_refuses_a_block_that_does_not_fit_and_is_freed_on_drop() {
        // Arrange
        let caps = InflightCaps::new(32, BLOCK);
        let held = caps.admit(Uuid::new_v4(), BLOCK).expect("first block");

        // Act
        let refused = caps.admit(Uuid::new_v4(), 1);
        drop(held);
        let admitted = caps.admit(Uuid::new_v4(), BLOCK);

        // Assert
        assert!(refused.is_none());
        assert!(admitted.is_some());
    }

    #[test]
    fn released_permits_leave_no_account_entry() {
        // Arrange
        let caps = InflightCaps::new(4, 1 << 30);
        let account = Uuid::new_v4();

        // Act
        drop(caps.admit(account, BLOCK).expect("admitted"));

        // Assert
        let counts = caps.lock();
        assert!(counts.blocks.is_empty());
        assert_eq!(counts.bytes, 0);
    }
}
