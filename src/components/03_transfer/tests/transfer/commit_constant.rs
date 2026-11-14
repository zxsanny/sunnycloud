//! AZ-3759 AC-3, the structural half of PT-03-2 (AC-4): the work of `commit_upload` does not grow with the file.
//! A 1 MiB and a 16 MiB commit make the same blob I/O: one head read of at most 64 KiB, no full or range read, no
//! block write or whole-file write, and the blob file is not touched. The timing half is `perf::commit_time`
//! (`NOT-RUN(HW-REF)` off REF-SRV).

use std::time::SystemTime;

use crate::commit_support::commit_request;
use crate::put_support::content;
use crate::start_support::{MIB, Rig};

/// 64 KiB: the most `read_head` returns (`commit_service.md`).
const HEAD_MAX: u32 = 64 * 1024;

/// The blob I/O of one commit and whether the blob file kept its length and modification time.
#[derive(Debug, PartialEq, Eq)]
struct CommitIo {
    opens: usize,
    heads: usize,
    largest_head_within_limit: bool,
    block_writes: usize,
    whole_writes: usize,
    file_untouched: bool,
}

fn file_stamp(rig: &Rig, blob: uuid::Uuid) -> (u64, SystemTime) {
    let meta = std::fs::metadata(sunnycloud_blobstore::blob_path(&rig.volume, blob)).expect("file");
    (meta.len(), meta.modified().expect("modification time"))
}

/// Uploads `len` bytes in full, then measures only the commit.
async fn commit_io(rig: &Rig, len: usize, seed: u64) -> CommitIo {
    let name = format!("constant-{len}.bin");
    let upload = rig.upload_bytes(&name, &name, content(seed, len)).await;
    rig.send_all(&upload).await;
    let stamp = file_stamp(rig, upload.blob);
    let (writes, completes) = (rig.writer.writes(), rig.writer.completes());
    rig.reader.reset();
    rig.commit(commit_request(&upload, rig.root, &name, None))
        .await
        .expect("committed");
    let (opens, heads, largest) = rig.reader.counts();
    CommitIo {
        opens,
        heads,
        largest_head_within_limit: largest <= HEAD_MAX,
        block_writes: rig.writer.writes() - writes,
        whole_writes: rig.writer.completes() - completes,
        file_untouched: file_stamp(rig, upload.blob) == stamp,
    }
}

#[tokio::test]
async fn commit_upload_of_one_and_sixteen_mebibytes_makes_the_same_blob_io() {
    // Arrange
    let rig = Rig::new().await;

    // Act
    let small = commit_io(&rig, MIB as usize, 31).await;
    let large = commit_io(&rig, 16 * MIB as usize, 32).await;

    // Assert
    let expected = CommitIo {
        opens: 0,
        heads: 1,
        largest_head_within_limit: true,
        block_writes: 0,
        whole_writes: 0,
        file_untouched: true,
    };
    assert_eq!((&small, &large), (&expected, &expected));
    rig.finish().await;
}
