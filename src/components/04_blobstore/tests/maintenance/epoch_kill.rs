//! AC-2, the kill: a process that writes the marker in a loop is killed, and the reads before and after the kill
//! only ever see a complete value.
//!
//! `child_writer` is the child half. It does nothing unless `SC_EPOCH_CHILD_ROOT` is set, which only
//! `a_process_killed_while_writing_leaves_the_old_or_the_new_value` does, on a fresh copy of this test binary.

use std::env;
use std::process::{Command, Stdio};

use contracts::EpochMarker;
use sunnycloud_blobstore::{BlobstoreConfig, FileEpochMarker};

use crate::support::Rig;

const CHILD_ROOT: &str = "SC_EPOCH_CHILD_ROOT";
const CHILD_WRITES: i64 = 1_000_000;
const SEEN_BEFORE_KILL: i64 = 25;
const MAX_POLLS: u32 = 1_000_000;

fn marker_at(config: &BlobstoreConfig) -> FileEpochMarker {
    FileEpochMarker::new(config).expect("marker")
}

#[tokio::test]
async fn child_writer() {
    let Ok(root) = env::var(CHILD_ROOT) else {
        return;
    };
    let mut config =
        crate::support::config(std::path::Path::new(&root), std::path::Path::new("unused"));
    config.blob_root = Some(root);
    let marker = marker_at(&config);
    for epoch in 1..=CHILD_WRITES {
        marker.write(epoch).await.expect("child write");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_process_killed_while_writing_leaves_the_old_or_the_new_value() {
    // Arrange
    let rig = Rig::new().await;
    let marker = marker_at(&rig.config);
    let mut child = Command::new(env::current_exe().expect("test binary"))
        .args(["--exact", "epoch_kill::child_writer", "--nocapture"])
        .env(CHILD_ROOT, &rig.volume)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("child process");

    // Act
    let mut seen = Vec::new();
    let mut polls = 0;
    while seen.last().is_none_or(|last| *last < SEEN_BEFORE_KILL) && polls < MAX_POLLS {
        polls += 1;
        if let Some(epoch) = marker
            .read()
            .await
            .expect("a read never fails on a cut value")
        {
            seen.push(epoch);
        }
        tokio::task::yield_now().await;
    }
    child.kill().expect("kill the child");
    child.wait().expect("reap the child");
    let after_kill = marker.read().await.expect("read after the kill");
    marker.write(i64::MAX).await.expect("write after the kill");
    let after_write = marker.read().await.expect("read after the next write");

    // Assert
    assert!(
        seen.last().is_some_and(|last| *last >= SEEN_BEFORE_KILL),
        "the child wrote: {seen:?}"
    );
    assert!(
        seen.windows(2).all(|pair| pair[0] <= pair[1]),
        "values only rise"
    );
    assert!(
        after_kill
            .is_some_and(|epoch| epoch >= *seen.last().unwrap_or(&0) && epoch <= CHILD_WRITES),
        "{after_kill:?}"
    );
    assert_eq!(after_write, Some(i64::MAX));
    rig.finish().await;
}
