//! AZ-3720 AC-3 (IT-02-12 with the real `BlobCommitEffect`, NFT-RES-34): 300 large uploads committed through
//! 03's `commit_upload`, each with an `Entry` or `Batch` failure injected after the real effects with probability
//! 0.3; after each, no committed revision names a missing or uncommitted blob and no committed blob is without
//! its revision. A failed commit leaves its session open; it is aborted, as a client would.

use contracts::CommitServiceError;

use crate::real_ports::{Fault, file_hash};
use crate::real_support::Real;

const PUBLISHES: usize = 300;

const DANGLING: &str = "SELECT count(*) FROM revisions r LEFT JOIN blobs b ON b.id = r.blob_id \
    WHERE b.id IS NULL OR b.state <> 'committed'";

const COMMITTED_WITHOUT_REVISION: &str = "SELECT count(*) FROM blobs b WHERE b.state = 'committed' \
    AND NOT EXISTS (SELECT 1 FROM revisions r WHERE r.blob_id = b.id)";

/// xorshift64: a fixed seed gives the same fates on every run; 3 in 20 of each failure kind.
struct Fates(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    Entry,
    Batch,
    Commit,
}

impl Fates {
    fn next(&mut self) -> Fate {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        match self.0 % 20 {
            0..=2 => Fate::Batch,
            3..=5 => Fate::Entry,
            _ => Fate::Commit,
        }
    }
}

#[tokio::test]
async fn no_committed_revision_names_an_uncommitted_blob_through_the_real_blob_effect() {
    // Arrange
    let real = Real::new().await;
    let mut fates = Fates(0x5eed_2026_1114);
    let mut probes = Vec::with_capacity(PUBLISHES);
    let mut counts = [0i64; 3];

    // Act
    for index in 0..PUBLISHES {
        let data = format!("content of file {index}").into_bytes();
        let (upload_id, request) = real.upload_one(&format!("f{index}.txt"), &data).await;
        let fate = fates.next();
        real.faults.set(match fate {
            Fate::Entry => Some(Fault::Entry(file_hash(&data))),
            Fate::Batch => Some(Fault::Batch),
            Fate::Commit => None,
        });
        let result = real.commit_upload(request).await;
        match (fate, &result) {
            (Fate::Commit, Ok(_)) => counts[0] += 1,
            (Fate::Entry, Err(CommitServiceError::UploadNotFound)) => counts[1] += 1,
            (Fate::Batch, Err(CommitServiceError::Unexpected(_))) => counts[2] += 1,
            other => panic!("commit {index}: {other:?}"),
        }
        if result.is_err() {
            real.abort(upload_id)
                .await
                .expect("abort the failed upload");
        }
        probes.push((
            index,
            real.count(DANGLING).await,
            real.count(COMMITTED_WITHOUT_REVISION).await,
        ));
    }
    real.faults.set(None);

    // Assert
    let violations: Vec<_> = probes
        .into_iter()
        .filter(|&(_, dangling, orphaned)| dangling != 0 || orphaned != 0)
        .collect();
    assert_eq!(
        violations,
        [],
        "(commit, dangling revisions, committed blobs without revision)"
    );
    assert!(counts.iter().all(|&count| count > 0), "{counts:?}");
    assert_eq!(
        real.count(&format!(
            "SELECT count(*) FROM revisions WHERE account_id = '{}'",
            real.account
        ))
        .await,
        counts[0]
    );
    real.finish().await;
}
