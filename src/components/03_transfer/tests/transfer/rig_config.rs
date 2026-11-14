//! The 02 and 04 configuration sections of the test rig.

use std::path::Path;
use std::time::Duration;

use sc_config::Secret;
use sunnycloud_blobstore::BlobstoreConfig;
use sunnycloud_tree::TreeConfig;

pub fn tree_config() -> TreeConfig {
    TreeConfig {
        tombstone_horizon: Duration::from_secs(30 * 86_400),
        listing_default_limit: 1000,
        listing_max_limit: 1000,
        cascade_batch_size: 1000,
        purge_batch_size: 1000,
        tree_maintenance_interval: Duration::from_secs(30),
        conflict_copy_time_zone: "UTC".to_owned(),
    }
}

pub fn blob_config(volume: &Path, key_file: &Path, free_reserve: u64) -> BlobstoreConfig {
    BlobstoreConfig {
        blob_root: Some(volume.to_string_lossy().into_owned()),
        retention: Duration::from_secs(7 * 86_400),
        orphan_sweep_age: Duration::from_secs(86_400),
        orphan_mode: "report".to_owned(),
        orphan_sweep_interval: Duration::from_secs(3600),
        blob_free_reserve: free_reserve,
        blob_free_reserve_percent: 100,
        gc_interval: Duration::from_secs(60),
        gc_batch: 100,
        gc_claim_lease: Duration::from_secs(60),
        gc_cycle_budget: Duration::from_secs(60),
        scrub_interval: Duration::from_secs(3600),
        scrub_cycle_budget: Duration::from_secs(60),
        scrub_max_rate: 1 << 20,
        scrub_pause_active_streams: 1,
        blob_sync_coalesce_delay: Duration::ZERO,
        blob_preallocate: false,
        blob_read_ahead_chunks: 1,
        blob_io_threads: None,
        master_key_file: Secret::new(key_file.to_string_lossy().into_owned()),
        master_key_ring: None,
        master_key_current: None,
    }
}
