//! An interrupted upload resumed by its `client_key` and committed (AZ-3738 AC-6, AT-03-1), and the committed key
//! kept for lost-commit recovery (AC-6b, `commit_service.md` 1.2.0). The restart is the worst case of a dropped
//! connection: blocks acknowledged after the last checkpoint are needed again.

use contracts::StartUploadOutcome;

use crate::commit_support::{commit_request, file_hash};
use crate::put_support::bit;
use crate::resume::send_lanes;
use crate::start_support::{MIB, Rig, TTL, now, request, started, ticks_past_ttl};

const HUNDRED_MIB: usize = 100 * MIB as usize;
/// AC-3 as changed 2026-11-11 (G-03-1, AZ-3731).
const RESEND_BOUND: u64 = 16 << 20;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commit_upload_after_a_resume_by_key_commits_the_source_file() {
    // Arrange
    let mut rig = Rig::new().await;
    let upload = rig.upload(HUNDRED_MIB, 101).await;
    let key = format!("put-101-{HUNDRED_MIB}");
    let forty_mib: Vec<u32> = (0..20).collect();
    send_lanes(&rig, &upload, &forty_mib).await;
    let (checkpoint, _, _) = rig.row(upload.id).await;
    rig.restart();

    // Act
    let declared = i64::try_from(HUNDRED_MIB).expect("size");
    let resumed = started(rig.start(request(rig.root, &key, declared, &key)).await);
    send_lanes(&rig, &upload, &resumed.need_blocks).await;
    let committed = rig
        .commit(commit_request(&upload, rig.root, "movie.bin", None))
        .await
        .expect("committed");

    // Assert
    assert_eq!(resumed.upload_id, upload.id, "same session");
    let from_row: Vec<u32> = (0..upload.block_count())
        .filter(|index| !bit(&checkpoint, *index))
        .collect();
    assert_eq!(resumed.need_blocks, from_row, "only the missing blocks");
    let resend: u64 = forty_mib
        .iter()
        .filter(|index| resumed.need_blocks.contains(index))
        .map(|index| upload.block(*index).len() as u64)
        .sum();
    println!("AZ-3738 AC-6: measured resend {resend} bytes");
    assert!(resend <= RESEND_BOUND, "resend {resend} bytes");
    let revision = rig.revision_row(committed.revision_id).await;
    assert_eq!(
        revision.content_hash,
        file_hash(&upload.data).as_bytes().to_vec()
    );
    rig.finish().await;
}

#[tokio::test]
async fn start_upload_after_a_commit_answers_the_committed_revision_until_the_ttl() {
    // Arrange
    let rig = Rig::new().await;
    let size = 6 * MIB;
    let upload = rig.upload(size as usize, 102).await;
    let key = format!("put-102-{size}");
    rig.send_all(&upload).await;
    let committed_at = now(&rig.clock);
    let committed = rig
        .commit(commit_request(&upload, rig.root, "kept.bin", None))
        .await
        .expect("committed");

    // Act
    let retry = rig
        .start(request(rig.root, "renamed.bin", size, &key))
        .await;
    let reserved = rig.reserved().await;
    let used = rig.used().await;
    let record = rig.session_record(upload.id).await;
    ticks_past_ttl(&rig.clock);
    let after_ttl = rig
        .start(request(rig.root, "renamed.bin", size, &key))
        .await;

    // Assert
    assert!(
        matches!(retry, Ok(StartUploadOutcome::NothingToSend { revision }) if revision == committed.revision_id),
        "{retry:?}"
    );
    assert_eq!((reserved, used), (0, size), "settled once at commit");
    let record = record.expect("committed-key record");
    assert_eq!(
        record.expires_at,
        committed_at.checked_add(TTL).expect("expiry")
    );
    let restarted = started(after_ttl);
    assert_ne!(
        restarted.upload_id, upload.id,
        "past the TTL the key starts anew"
    );
    rig.finish().await;
}
