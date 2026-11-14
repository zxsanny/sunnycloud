//! `ReservationSource` (AZ-3756 AC-5; IT-03-12): after a seeded random history of starts, stages, aborts, commits
//! and expiries on one account, `open_reserved_bytes` equals 07a's reserved bytes at every step, and
//! `QuotaAdmin::recompute` fed by 02's `LiveUsageSource` and this source finds no drift and changes nothing.

use std::sync::Arc;
use std::time::Duration;

use contracts::{Janitor, LiveUsageSource, QuotaAdmin, ReservationSource, UploadError};
use sc_clock::Clock;
use sunnycloud_quota::{PgQuotaAdmin, QuotaConfig};
use sunnycloud_transfer::SessionReaper;
use sunnycloud_tree::PgTreeReader;
use uuid::Uuid;

use crate::commit_support::commit_request;
use crate::janitor_support::short_config;
use crate::put_support::{Upload, content};
use crate::rig_config::tree_config;
use crate::start_support::{GIB, MIB, Rig};

const ROUNDS: u64 = 60;

/// A xorshift sequence: the history is random in shape and the same on every run.
struct Seeded(u64);

impl Seeded {
    fn below(&mut self, bound: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % bound
    }
}

#[tokio::test]
async fn open_reserved_bytes_matches_the_ledger_through_a_random_history() {
    // Arrange
    let rig = Rig::with_config(short_config()).await;
    let reaper = SessionReaper::new(Arc::clone(&rig.service));
    let admin = PgQuotaAdmin::new(
        rig.fixture.api.clone(),
        QuotaConfig {
            default_quota_bytes: 10 * GIB,
        },
    );
    let live = PgTreeReader::new(rig.fixture.api.clone(), &tree_config());
    let source: &dyn ReservationSource = rig.service.as_ref();
    let mut rng = Seeded(0x5eed_1234_abcd_0001);
    let mut open: Vec<Upload> = Vec::new();
    let mut mismatches = Vec::new();

    // Act
    for round in 0..ROUNDS {
        let size = (1 + rng.below(3) as usize) * MIB as usize - rng.below(1000) as usize;
        match rng.below(5) {
            0 => open.push(rig.upload(size, 500 + round).await),
            1 => {
                let data = content(600 + round, rng.below(4096) as usize);
                rig.stage(Uuid::new_v4(), &data).await.expect("staged");
            }
            2 if !open.is_empty() => {
                let upload = open.remove(rng.below(open.len() as u64) as usize);
                let aborted = rig.service.abort_upload(rig.account, upload.id).await;
                assert!(
                    matches!(aborted, Ok(()) | Err(UploadError::UploadNotFound)),
                    "{aborted:?}"
                );
            }
            3 => {
                let name = format!("commit-{round}.bin");
                let upload = rig
                    .upload_bytes(&name, &name, content(700 + round, size))
                    .await;
                rig.send_all(&upload).await;
                rig.commit(commit_request(&upload, rig.root, &name, None))
                    .await
                    .expect("committed");
            }
            _ => {
                reaper.run_cycle(rig.clock.now_utc()).await.expect("reap");
            }
        }
        rig.clock.tick(Duration::from_millis(150));
        let ours = source
            .open_reserved_bytes(rig.account)
            .await
            .expect("source");
        let ledger = admin.view(rig.account).await.expect("view").reserved;
        if ours != ledger {
            mismatches.push((round, ours, ledger));
        }
    }
    let live_bytes = live.live_bytes(rig.account).await.expect("live bytes");
    let reserved = source
        .open_reserved_bytes(rig.account)
        .await
        .expect("source");
    let before = admin.view(rig.account).await.expect("view");
    let drift = admin
        .recompute_columns(rig.account, live_bytes, reserved, false)
        .await
        .expect("drift check");
    admin
        .recompute(rig.account, live_bytes, reserved, true)
        .await
        .expect("repair pass");
    let after = admin.view(rig.account).await.expect("view");

    // Assert
    assert!(mismatches.is_empty(), "{mismatches:?}");
    assert_eq!(drift.used.stored, drift.used.computed);
    assert_eq!(drift.reserved.stored, drift.reserved.computed);
    assert_eq!(after, before, "recompute changed nothing");
    assert!(
        before.reserved > 0 || before.used > 0,
        "the history did something"
    );
    drop(reaper);
    drop(admin);
    drop(live);
    rig.finish().await;
}
