//! AZ-3871 AC-2, memory part. Its own test binary, so no other test shares the process while RSS is measured.

mod decoder_support;

use std::future::pending;

use decoder_support::{HostilePeer, hostile_cases};
use sunnycloud_worker::DecoderConn;

const ROUNDS: usize = 20;
const LIMIT_KIB: u64 = 10 * 1024;

fn resident_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kib| kib.parse().ok())
        .expect("a VmRSS line in kB")
}

#[tokio::test]
async fn hostile_replies_grow_the_process_by_less_than_ten_mebibytes() {
    // Arrange
    let dir = tempfile::tempdir().expect("temporary folder");
    let mut sockets = Vec::new();
    for (index, (_, reply, _)) in hostile_cases().into_iter().enumerate() {
        let path = dir.path().join(format!("decoder-{index}.sock"));
        sockets.push((path.clone(), HostilePeer::start(&path, reply)));
    }
    let before = resident_kib();

    // Act
    for _ in 0..ROUNDS {
        for (path, _) in &sockets {
            let mut conn = DecoderConn::connect(path).await.expect("connect");
            let result = conn.probe("/scratch/job", &mut |_| {}, pending()).await;
            assert!(result.is_err());
        }
    }

    // Assert
    let grown = resident_kib().saturating_sub(before);
    assert!(
        grown < LIMIT_KIB,
        "resident memory grew by {grown} KiB over {} hostile replies",
        ROUNDS * sockets.len()
    );
}
