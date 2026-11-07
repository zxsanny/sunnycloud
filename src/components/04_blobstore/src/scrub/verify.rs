//! The check of one blob file: every chunk tag, the final-flag chunk and the file length, through the reader's own
//! chunk verification. Each chunk read waits for the activity gauge and is charged to the run's byte rate.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use contracts::BlobError;
use sc_clock::{Clock, MonotonicInstant};
use zeroize::Zeroize;

use super::pace::Pacer;
use super::scrub_repo::Candidate;
use crate::activity::ActivityGauge;
use crate::error::blob_error_from_io;
use crate::keyring::MasterKeyring;
use crate::layout::{BlobLayout, TAG_LEN};
use crate::reader::ReadProbe;
use crate::reader::file::{OpenedBlob, blob_ids};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Verdict {
    Clean,
    Corrupt,
    Missing,
}

/// Why a check ended without a verdict.
pub(super) enum Interrupted {
    /// The gauge stayed above the limit until the step's deadline.
    PausedPastDeadline,
    /// The clock was shut down.
    Cancelled,
    /// A failure that is not a finding: an unknown master key, an I/O error other than a missing file.
    Failed(BlobError),
}

/// The throttle of one step: pause under load, then the rate cap. `slept` is the time spent in rate sleeps.
pub(super) struct Gate<'a> {
    pub clock: &'a dyn Clock,
    pub gauge: Option<&'a ActivityGauge>,
    pub pause_above: u64,
    pub deadline: Option<MonotonicInstant>,
    pub pacer: &'a mut Pacer,
    pub slept: Duration,
}

impl Gate<'_> {
    /// Returns when the gauge is at most the limit; at once without a gauge.
    async fn calm(&self) -> Result<(), Interrupted> {
        let Some(gauge) = self.gauge else {
            return Ok(());
        };
        if gauge.active() <= self.pause_above {
            return Ok(());
        }
        let Some(deadline) = self.deadline else {
            gauge.wait_until_at_most(self.pause_above).await;
            return Ok(());
        };
        tokio::select! {
            () = gauge.wait_until_at_most(self.pause_above) => Ok(()),
            woke = self.clock.sleep_until_monotonic(deadline) => match woke {
                Ok(()) => Err(Interrupted::PausedPastDeadline),
                Err(_) => Err(Interrupted::Cancelled),
            },
        }
    }

    async fn pace(&mut self, bytes: u64) -> Result<(), Interrupted> {
        let now = self.clock.monotonic();
        let free_at = self.pacer.charge(now, bytes);
        let wait = free_at.saturating_duration_since(now);
        if wait.is_zero() {
            return Ok(());
        }
        self.slept += wait;
        self.clock
            .sleep_until_monotonic(free_at)
            .await
            .map_err(|_| Interrupted::Cancelled)
    }
}

pub(super) struct Files<'a> {
    pub root: &'a Path,
    pub keyring: &'a Arc<MasterKeyring>,
    pub probe: &'a Arc<ReadProbe>,
}

fn blocking_failed(blob_id: uuid::Uuid) -> Interrupted {
    Interrupted::Failed(blob_error_from_io(
        io::Error::other("blocking task did not finish"),
        blob_ids(blob_id),
    ))
}

impl Files<'_> {
    pub async fn verify(
        &self,
        candidate: &Candidate,
        gate: &mut Gate<'_>,
    ) -> Result<Verdict, Interrupted> {
        let blob_id = candidate.id;
        let layout = u64::try_from(candidate.size)
            .ok()
            .and_then(|size| BlobLayout::new(size).ok())
            .ok_or(Interrupted::Failed(BlobError::OutOfRange))?;
        gate.calm().await?;
        let (root, keyring, probe) = (
            self.root.to_path_buf(),
            Arc::clone(self.keyring),
            Arc::clone(self.probe),
        );
        let (wrapped_key, master_key_id) = (candidate.wrapped_key.clone(), candidate.master_key_id);
        let opened = tokio::task::spawn_blocking(move || {
            let key_material = (wrapped_key.as_slice(), master_key_id);
            OpenedBlob::open(&root, &keyring, probe, key_material, blob_id, layout)
        })
        .await
        .map_err(|_| blocking_failed(blob_id))?;
        let blob = match opened {
            Ok(blob) => Arc::new(blob),
            Err(error) => return finding_of(error),
        };
        for index in 0..layout.chunk_count() {
            gate.calm().await?;
            let reader = Arc::clone(&blob);
            let read = tokio::task::spawn_blocking(move || reader.read_chunk(index))
                .await
                .map_err(|_| blocking_failed(blob_id))?;
            match read {
                Ok(mut plain) => plain.zeroize(),
                Err(error) => return finding_of(error),
            }
            let sealed = layout.plain_len(index).unwrap_or_default() + TAG_LEN;
            gate.pace(sealed).await?;
        }
        Ok(Verdict::Clean)
    }
}

/// `Corrupt` and `BlobMissing` are what a scrub is for; any other failure ends the step.
fn finding_of(error: BlobError) -> Result<Verdict, Interrupted> {
    match error {
        BlobError::Corrupt => Ok(Verdict::Corrupt),
        BlobError::BlobMissing => Ok(Verdict::Missing),
        other => Err(Interrupted::Failed(other)),
    }
}
