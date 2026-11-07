use std::sync::Arc;
use std::time::Duration;

use super::support::Rig;

const FILES: usize = 10;
const WRITES: usize = 100;

#[tokio::test(flavor = "multi_thread")]
async fn a_hundred_writes_over_ten_files_share_few_flushes_and_each_file_is_synced_after_its_last_write()
 {
    let rig = Arc::new(Rig::new(Duration::ZERO));
    rig.hook.delay_flushes(Duration::from_millis(5));
    let files: Vec<_> = (0..FILES).map(|_| rig.scratch.file()).collect();

    let waiters: Vec<_> = (0..WRITES)
        .map(|n| {
            let rig = Arc::clone(&rig);
            let (id, file) = files[n % FILES].clone();
            tokio::spawn(async move {
                let written = rig.write_and_register(id, &file, (n / FILES) as u64 * 4096);
                rig.barrier.await_durable(written.token).await?;
                Ok::<_, contracts::BlobError>((id, written.stamp))
            })
        })
        .collect();

    let mut last_write = std::collections::HashMap::new();
    for waiter in waiters {
        let (id, stamp) = waiter
            .await
            .expect("task finishes")
            .expect("every write is acknowledged");
        let records = rig.hook.records();
        assert!(
            records
                .iter()
                .any(|record| record.blob_id == id && record.stamp > stamp),
            "acknowledged before a sync of the file began after its write"
        );
        let last = last_write.entry(id).or_insert(stamp);
        *last = (*last).max(stamp);
    }

    let flushes = rig.hook.flush_count();
    assert!(flushes <= 25, "{flushes} flushes for {WRITES} writes");
    let records = rig.hook.records();
    assert_eq!(last_write.len(), FILES);
    for (id, last) in last_write {
        let final_sync = records
            .iter()
            .filter(|record| record.blob_id == id)
            .map(|record| record.stamp)
            .max()
            .expect("the file was synced");
        assert!(
            final_sync > last,
            "file {id} not synced after its last write"
        );
    }
}
