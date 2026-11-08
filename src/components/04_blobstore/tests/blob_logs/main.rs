//! The log lines of the blob I/O paths (AZ-3748): ERROR for a corrupt chunk on read, an I/O error on a block
//! write and a failed group sync, WARN for the free-space floor, ERROR for an aborted GC sweep; one line each, with
//! the blob id and no path or key bytes; and no line per block or chunk, so a 64 MiB transfer logs what a 2 MiB one
//! does. This binary holds this one test and installs the capturing subscriber globally, because the group flush
//! runs on a spawned task, which a scoped default of the test's thread would not see.

#[allow(dead_code)]
#[path = "../scrub/support.rs"]
mod support;

use std::fs;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use contracts::{BlobError, BlobReader, BlobWriter, StreamError};
use sc_clock::Timestamp;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use sunnycloud_blobstore::{FileBlobWriter, SyncBarrier, SyncHook};
use support::{KEY, MIB, Rig};
use tracing::Level;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<String>>>);

impl Lines {
    fn all(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The blobstore's own lines after `mark`; sqlx and audit lines of the same calls are other components'.
    fn since(&self, mark: usize) -> Vec<String> {
        self.all()[mark..]
            .iter()
            .filter(|line| line.contains("\"target\":\"sunnycloud_blobstore"))
            .cloned()
            .collect()
    }
}

impl LineSink for Lines {
    fn write_line(&self, line: &str) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_owned());
        Ok(())
    }
}

async fn read_all(rig: &Rig, blob: &contracts::BlobRef) -> Result<usize, StreamError> {
    let mut stream = rig
        .reader
        .open(
            blob.blob_id,
            &blob.wrapped_key,
            blob.master_key_id,
            blob.size,
            None,
        )
        .await
        .expect("open");
    let mut total = 0;
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        total += item?.len();
    }
    Ok(total)
}

fn only_line(lines: &[String], level: &str, message: &str, blob_id: Uuid) {
    assert_eq!(lines.len(), 1, "{message}: {lines:?}");
    assert!(
        lines[0].contains(&format!("\"level\":\"{level}\"")),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains(message), "{}", lines[0]);
    assert!(lines[0].contains(&blob_id.to_string()), "{}", lines[0]);
}

#[tokio::test(flavor = "multi_thread")]
async fn each_blob_io_event_is_one_line_with_ids_only_and_transfers_log_nothing_per_block() {
    // Arrange
    let rig = Rig::new().await;
    let lines = Lines::default();
    let (capture, _) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::DEBUG,
            component: Component::Api,
        },
        sc_testdb::clock_at_start(),
        Arc::new(lines.clone()),
    );
    tracing::subscriber::set_global_default(capture).expect("first subscriber of this process");
    let small = rig.store(2 * MIB, 1).await;
    let large = rig.store(64 * MIB, 2).await;

    // Act: AC-5, the same transfer at two sizes
    let mark = lines.all().len();
    assert_eq!(read_all(&rig, &small).await.expect("small"), 2 * MIB);
    let small_lines = lines.since(mark).len();
    let mark = lines.all().len();
    assert_eq!(read_all(&rig, &large).await.expect("large"), 64 * MIB);
    let large_lines = lines.since(mark).len();

    // Act: a corrupt chunk on read
    let corrupt = rig.store(64 * 1024, 3).await;
    rig.flip_bit(corrupt.blob_id);
    let mark = lines.all().len();
    let corrupted = rig
        .reader
        .open(
            corrupt.blob_id,
            &corrupt.wrapped_key,
            corrupt.master_key_id,
            corrupt.size,
            None,
        )
        .await
        .map(drop);
    let corrupt_lines = lines.since(mark);

    // Act: an I/O error on a block write (the blob path is a folder)
    let handle = {
        let plan = rig.writer.plan(rig.account, 3 * MIB as i64).expect("plan");
        rig.writer.create_file(plan, false).await.expect("create")
    };
    let path = rig.path(handle.plan.blob_id);
    fs::remove_file(&path).expect("remove the file");
    fs::create_dir(&path).expect("folder in its place");
    let mark = lines.all().len();
    let write = rig
        .writer
        .write_block(&handle, 0, Bytes::from(vec![9; 2 * MIB]))
        .await;
    let write_lines = lines.since(mark);

    // Act: a failed group sync
    let hook = SyncHook::new();
    let hooked = FileBlobWriter::new(
        &rig.config,
        Arc::clone(&rig.keyring),
        SyncBarrier::with_hook(Duration::ZERO, Arc::clone(&hook)),
    )
    .expect("writer");
    let plan = hooked.plan(rig.account, 3 * MIB as i64).expect("plan");
    let synced = hooked.create_file(plan, false).await.expect("create");
    let token = hooked
        .write_block(&synced, 0, Bytes::from(vec![9; 2 * MIB]))
        .await
        .expect("block");
    hook.fail_next_flushes(1);
    let mark = lines.all().len();
    let sync = hooked.await_durable(token).await;
    let sync_lines = lines.since(mark);

    // Act: the free-space floor
    let mut tight = rig.config.clone();
    tight.blob_free_reserve = u64::MAX;
    let floor_writer = FileBlobWriter::new(
        &tight,
        Arc::clone(&rig.keyring),
        SyncBarrier::new(Duration::ZERO),
    )
    .expect("writer");
    let plan = floor_writer.plan(rig.account, 1024).expect("plan");
    let floor_id = plan.blob_id;
    let mark = lines.all().len();
    let floor = floor_writer.create_file(plan, false).await;
    let floor_lines = lines.since(mark);

    // Act: a GC sweep that cannot compute its lease
    let sweeper = rig.sweeper(None);
    let mark = lines.all().len();
    let swept = sweeper
        .run_cycle(Timestamp::from_unix_micros(i64::MAX))
        .await;
    let sweep_lines = lines.since(mark);

    // Assert
    assert_eq!(small_lines, large_lines, "no line per block or chunk");
    assert!(
        matches!(corrupted, Err(BlobError::Corrupt)),
        "{corrupted:?}"
    );
    only_line(
        &corrupt_lines,
        "error",
        "blob corrupt on read",
        corrupt.blob_id,
    );
    assert!(matches!(write, Err(BlobError::Io(_))), "{write:?}");
    only_line(
        &write_lines,
        "error",
        "blob write failed",
        handle.plan.blob_id,
    );
    assert!(matches!(sync, Err(BlobError::Io(_))), "{sync:?}");
    only_line(
        &sync_lines,
        "error",
        "blob group sync failed",
        synced.plan.blob_id,
    );
    assert!(matches!(floor, Err(BlobError::FreeSpaceFloor)), "{floor:?}");
    only_line(&floor_lines, "warn", "free-space floor", floor_id);
    assert!(swept.is_err());
    assert_eq!(sweep_lines.len(), 2, "{sweep_lines:?}");
    assert!(sweep_lines[0].contains("gc cycle started"));
    assert!(sweep_lines[1].contains("\"level\":\"error\""));
    assert!(sweep_lines[1].contains("gc sweep aborted"));
    let key_hex: String = KEY.iter().map(|byte| format!("{byte:02x}")).collect();
    let volume = rig.volume.to_string_lossy().into_owned();
    for line in lines.all() {
        assert!(!line.contains(&volume), "{line}");
        assert!(!line.contains(&key_hex), "{line}");
    }
    drop((hooked, floor_writer, sweeper));
    rig.finish().await;
}
