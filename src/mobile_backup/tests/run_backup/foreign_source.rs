//! The loop driven through the adapter of the `ForeignAssetSource` twin, over a Rust host: the path Kotlin and Swift
//! take, minus the JVM. A host error is a value, never a panic.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use mobile_backup::{
    AccessLevel, AssetDescriptor, AssetSource, BackupOutcome, ChangeToken, ForeignAssetSource,
    ForeignOpened, RunError, SourceError, Trigger,
};
use sync_core::{FileIdentity, FileSource, Foreign, ForeignFileSource, HostError};

use crate::support::source::FakeSource;
use crate::support::{Rig, ctx};

struct HostFile(Arc<dyn FileSource>);

impl ForeignFileSource for HostFile {
    fn len(&self) -> Result<u64, HostError> {
        self.0.len().map_err(|_| HostError::Failed)
    }

    fn read_at(&self, offset: u64, len: u32) -> Result<Vec<u8>, HostError> {
        let bytes: Bytes = self.0.read_at(offset, len).map_err(|_| HostError::Failed)?;
        Ok(bytes.to_vec())
    }

    fn identity(&self) -> Result<FileIdentity, HostError> {
        self.0.identity().map_err(|_| HostError::Failed)
    }
}

/// A host over the fake photos that can fail its calls as a throwing Kotlin implementation would.
struct Host {
    photos: Arc<FakeSource>,
    open_errors: Mutex<HashMap<i64, HostError>>,
    access_fails: AtomicBool,
    listing_fails: AtomicBool,
}

impl Host {
    fn new(photos: Arc<FakeSource>) -> Arc<Self> {
        Arc::new(Self {
            photos,
            open_errors: Mutex::default(),
            access_fails: AtomicBool::new(false),
            listing_fails: AtomicBool::new(false),
        })
    }

    fn as_source(self: &Arc<Self>) -> Arc<dyn AssetSource> {
        Arc::new(Foreign(Arc::clone(self) as Arc<dyn ForeignAssetSource>))
    }
}

fn failed(_: SourceError) -> HostError {
    HostError::Failed
}

impl ForeignAssetSource for Host {
    fn assets_after(
        &self,
        token: ChangeToken,
        limit: u32,
    ) -> Result<Vec<AssetDescriptor>, HostError> {
        if self.listing_fails.load(Ordering::SeqCst) {
            return Err(HostError::Failed);
        }
        self.photos.assets_after(&token, limit).map_err(failed)
    }

    fn current_token(&self) -> Result<ChangeToken, HostError> {
        self.photos.current_token().map_err(failed)
    }

    fn list_visible_ids(&self, page: u32) -> Result<Vec<i64>, HostError> {
        self.photos.list_visible_ids(page).map_err(failed)
    }

    fn visible_count(&self) -> Result<u64, HostError> {
        self.photos.visible_count().map_err(failed)
    }

    fn describe(&self, asset_ids: Vec<i64>) -> Result<Vec<AssetDescriptor>, HostError> {
        self.photos.describe(&asset_ids).map_err(failed)
    }

    fn open_read(&self, asset_id: i64) -> Result<ForeignOpened, HostError> {
        if let Some(error) = self.open_errors.lock().expect("errors").get(&asset_id) {
            return Err(*error);
        }
        let opened = self
            .photos
            .open_read(asset_id)
            .map_err(|error| match error {
                mobile_backup::OpenError::Gone => HostError::NotFound,
                mobile_backup::OpenError::Failed => HostError::Failed,
            })?;
        Ok(ForeignOpened {
            file: Arc::new(HostFile(opened.file)),
            location_redacted: opened.location_redacted,
        })
    }

    fn access_level(&self) -> Result<AccessLevel, HostError> {
        if self.access_fails.load(Ordering::SeqCst) {
            return Err(HostError::Failed);
        }
        self.photos.access_level().map_err(failed)
    }
}

fn rig_with_six_assets() -> Rig {
    let rig = Rig::new(8, 50);
    for id in 1..=6 {
        rig.source.add(id, 1024);
    }
    rig
}

#[tokio::test]
async fn host_errors_on_open_end_the_asset_gone_or_failed_and_the_loop_goes_on() {
    // Arrange
    let rig = rig_with_six_assets();
    rig.source.vanish_on_open(2);
    let host = Host::new(Arc::clone(&rig.source));
    host.open_errors
        .lock()
        .expect("errors")
        .extend([(3, HostError::PermissionDenied), (4, HostError::Failed)]);
    host.photos.set_redacted(true);

    // Act
    let outcome = rig
        .backup
        .run_backup(ctx(Trigger::FirstSync), host.as_source())
        .await
        .expect("an outcome");
    let hashed = rig.api.record().hashed.len();
    let first = rig.core.status().expect("status");
    host.open_errors.lock().expect("errors").clear();
    let again = rig
        .backup
        .run_backup(ctx(Trigger::AppOpen), host.as_source())
        .await
        .expect("an outcome");

    // Assert
    assert_eq!(outcome, BackupOutcome::Completed);
    assert_eq!(again, BackupOutcome::Completed);
    let status = rig.core.status().expect("status");
    assert_eq!(
        (
            status.uploaded,
            status.skipped.gone,
            status.skipped.failed,
            status.pending
        ),
        (3, 2, 1, 0)
    );
    assert_eq!(
        first.last_error.expect("an error").message,
        "the host could not open the original"
    );
    assert!(first.location_redacted, "the flag crossed the adapter");
    assert_eq!(
        rig.api.record().hashed.len(),
        hashed,
        "a failed asset was retried"
    );
    assert_eq!(rig.server.file_count(), 3);
}

#[tokio::test]
async fn a_failing_access_level_or_listing_ends_the_run_with_a_source_error_and_changes_nothing() {
    // Arrange
    let rig = rig_with_six_assets();
    let host = Host::new(Arc::clone(&rig.source));
    host.access_fails.store(true, Ordering::SeqCst);

    // Act
    let by_access = rig
        .backup
        .run_backup(ctx(Trigger::FirstSync), host.as_source())
        .await;
    host.access_fails.store(false, Ordering::SeqCst);
    host.listing_fails.store(true, Ordering::SeqCst);
    let by_listing = rig
        .backup
        .run_backup(ctx(Trigger::FirstSync), host.as_source())
        .await;
    host.listing_fails.store(false, Ordering::SeqCst);
    let healed = rig
        .backup
        .run_backup(ctx(Trigger::FirstSync), host.as_source())
        .await
        .expect("an outcome");

    // Assert
    assert!(
        matches!(by_access, Err(RunError::Source(_))),
        "{by_access:?}"
    );
    assert!(
        matches!(by_listing, Err(RunError::Source(_))),
        "{by_listing:?}"
    );
    assert_eq!(healed, BackupOutcome::Completed);
    assert_eq!(rig.server.file_count(), 6);
}
