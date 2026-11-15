//! The idle flush (`ProgressTable::flush_idle` of the component description; it lives on the service because the
//! table holds no database handle). An entry with no block in flight and none acknowledged for
//! `checkpoint_interval` gets its unsaved progress written to its row as a checkpoint (`received`,
//! `block_hashes`, `last_checkpoint_at` and the rolled `expires_at`; a committed, expired or vanished row is not
//! written) and is evicted. An entry with nothing unsaved is evicted without a write.
//!
//! The shutdown flush ([`PgUploadService::flush_all`]) is the same work for every entry, idle or not and with a
//! block in flight or not: `serve` runs it after the connections are drained, so no acknowledgement can follow.
//!
//! A block that starts on the entry between the idle check and the eviction is written to the evicted entry; at
//! worst its bit misses the row and the client sends it again, as after a restart.

use std::time::Duration;

use contracts::Fault;
use sc_clock::Timestamp;
use sc_db::Scope;
use thiserror::Error;

use crate::checkpoint::{CheckpointMark, rolled_expiry};
use crate::put_block::CheckpointRun;
use crate::reaper::run_fault;
use crate::session_repo::{self, Progress};
use crate::upload::PgUploadService;

#[derive(Debug, Error)]
enum FlushFault {
    #[error("checkpoint expiry overflows the timestamp range")]
    ExpiryOverflow,
}

/// Which entries a flush takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entries {
    /// No block in flight and none acknowledged for `checkpoint_interval`.
    Idle,
    /// Every entry: the shutdown flush.
    All,
}

impl PgUploadService {
    /// Checkpoints and evicts the idle entries; returns how many were evicted. `now` stamps the rows.
    pub(crate) async fn flush_idle(&self, now: Timestamp) -> Result<u64, Fault> {
        self.flush(now, Entries::Idle).await
    }

    /// Saves every entry's unsaved progress now and evicts it, whatever its idle time; returns how many were
    /// evicted. `serve` calls it at shutdown, after the connections are drained and the scheduler stopped. A
    /// checkpoint that a block already runs for the entry is left to that block.
    pub async fn flush_all(&self, now: Timestamp) -> Result<u64, Fault> {
        self.flush(now, Entries::All).await
    }

    async fn flush(&self, now: Timestamp, entries: Entries) -> Result<u64, Fault> {
        let at = self.clock.monotonic();
        let idle_for = match entries {
            Entries::Idle => self.config.checkpoint_interval,
            Entries::All => Duration::ZERO,
        };
        let mut evicted = 0u64;
        for (upload_id, session) in self.progress.entries() {
            if entries == Entries::Idle && session.lock().in_flight_count() > 0 {
                continue;
            }
            let claimed = session.checkpoint().claim_idle(at, idle_for);
            let Some(taken) = claimed else {
                continue;
            };
            let run = CheckpointRun::new(&session, taken);
            if taken > 0 {
                let expires_at = rolled_expiry(now, self.config.upload_session_ttl)
                    .ok_or_else(|| Box::new(FlushFault::ExpiryOverflow) as Fault)?;
                let (received, block_hashes) = {
                    let progress = session.lock();
                    (
                        progress.received_bytes().to_vec(),
                        progress.block_hash_bytes().to_vec(),
                    )
                };
                let account = session.info().account;
                let row = Progress {
                    id: upload_id,
                    account,
                    received: &received,
                    block_hashes: &block_hashes,
                    now,
                    expires_at,
                };
                self.db
                    .run(Scope::Account(account), async |tx| -> Result<u64, Fault> {
                        Ok(session_repo::checkpoint(tx, &row).await?)
                    })
                    .await
                    .map_err(run_fault)?;
                run.succeeded(CheckpointMark { at, expires_at });
            } else {
                drop(run);
            }
            self.evict(upload_id);
            evicted += 1;
        }
        Ok(evicted)
    }
}
