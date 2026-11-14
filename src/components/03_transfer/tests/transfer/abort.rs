//! `abort_upload` and the refusals of `get_upload` (AZ-3736 AC-3, AC-4; IT-03-10 abort line, ST-03-1): an abort
//! releases exactly the row's reservation once and removes the session, blob row and file; expired and foreign
//! sessions are not found, in memory and after a restart.

use contracts::UploadError;
use sc_observability::{BuildInfo, Metrics};
use uuid::Uuid;

use crate::start_support::{MIB, Rig, close, ticks_past_ttl};

const TEN_MIB: usize = 10 * MIB as usize;

#[tokio::test]
async fn abort_upload_releases_exactly_its_reservation_once() {
    // Arrange
    let rig = Rig::new().await;
    let upload = rig.upload(TEN_MIB, 80).await;
    for index in 0..2 {
        rig.put(&upload, index)
            .await
            .expect("block before the abort");
    }
    let reserved_before = rig.reserved().await;
    let files_before = rig.files();

    // Act
    let first = rig.service.abort_upload(rig.account, upload.id).await;
    let reserved_after_first = rig.reserved().await;
    let second = rig.service.abort_upload(rig.account, upload.id).await;
    let state = rig.service.get_upload(rig.account, upload.id).await;
    let put = rig.put(&upload, 2).await;

    // Assert
    assert!(first.is_ok(), "{first:?}");
    assert_eq!(reserved_before - reserved_after_first, 10 * MIB);
    assert!(
        matches!(second, Err(UploadError::UploadNotFound)),
        "{second:?}"
    );
    assert_eq!(
        rig.reserved().await,
        reserved_after_first,
        "the repeat releases nothing"
    );
    assert_eq!(rig.sessions().await, 0);
    assert_eq!(rig.uploading_blobs().await, 0);
    assert_eq!((files_before, rig.files()), (1, 0), "file discarded");
    assert!(
        matches!(state, Err(UploadError::UploadNotFound)),
        "{state:?}"
    );
    assert!(matches!(put, Err(UploadError::UploadNotFound)), "{put:?}");
    rig.finish().await;
}

#[tokio::test]
async fn abort_upload_after_a_restart_needs_no_progress_entry() {
    // Arrange
    let mut rig = Rig::new().await;
    let upload = rig.upload(TEN_MIB, 81).await;
    rig.put(&upload, 0).await.expect("block before the restart");
    rig.restart();

    // Act
    let aborted = rig.service.abort_upload(rig.account, upload.id).await;

    // Assert
    assert!(aborted.is_ok(), "{aborted:?}");
    assert_eq!(rig.reserved().await, 0);
    assert_eq!((rig.sessions().await, rig.uploading_blobs().await), (0, 0));
    assert_eq!(rig.files(), 0);
    rig.finish().await;
}

#[tokio::test]
async fn abort_upload_counts_an_aborted_session() {
    // Arrange
    let rig = Rig::new().await;
    let metrics = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics");
    let service = rig
        .instance()
        .with_metrics(&metrics)
        .expect("upload series");
    let request = crate::start_support::request(rig.root, "counted.bin", 4 * MIB, "counted");
    let id =
        crate::start_support::started(service.start_upload(rig.account, request).await).upload_id;

    // Act
    service.abort_upload(rig.account, id).await.expect("abort");
    let rendered = metrics.render().await.expect("render");

    // Assert
    assert!(
        rendered.contains("upload_sessions_total{result=\"aborted\"} 1"),
        "{rendered}"
    );
    assert!(rendered.contains("upload_sessions_active 0"), "{rendered}");
    drop(service);
    rig.finish().await;
}

#[tokio::test]
async fn get_upload_an_expired_or_foreign_session_is_not_found() {
    // Arrange
    let mut rig = Rig::new().await;
    let upload = rig.upload(TEN_MIB, 82).await;
    let other = Uuid::new_v4();
    let mut conn = rig.owner().await;
    crate::support::account(&mut conn, other).await;
    close(conn).await;
    let reserved = rig.reserved().await;

    // Act
    let foreign = rig.service.get_upload(other, upload.id).await;
    let foreign_abort = rig.service.abort_upload(other, upload.id).await;
    rig.restart();
    let foreign_loaded = rig.service.get_upload(other, upload.id).await;
    let own_loaded = rig.service.get_upload(rig.account, upload.id).await;
    ticks_past_ttl(&rig.clock);
    let expired = rig.service.get_upload(rig.account, upload.id).await;
    rig.restart();
    let expired_loaded = rig.service.get_upload(rig.account, upload.id).await;
    let expired_abort = rig.service.abort_upload(rig.account, upload.id).await;

    // Assert
    for result in [foreign, foreign_loaded, expired, expired_loaded] {
        assert!(
            matches!(result, Err(UploadError::UploadNotFound)),
            "{result:?}"
        );
    }
    for result in [foreign_abort, expired_abort] {
        assert!(
            matches!(result, Err(UploadError::UploadNotFound)),
            "{result:?}"
        );
    }
    assert!(own_loaded.is_ok(), "{own_loaded:?}");
    assert_eq!(rig.reserved().await, reserved, "nothing released");
    assert_eq!(rig.sessions().await, 1, "the expired row is the reaper's");
    rig.finish().await;
}

#[tokio::test]
async fn an_expired_session_dropped_from_memory_lowers_the_active_gauge() {
    // Arrange
    let rig = Rig::new().await;
    let metrics = Metrics::new(BuildInfo {
        version: "test",
        commit: "test",
        test_clock: true,
    })
    .expect("metrics");
    let service = rig
        .instance()
        .with_metrics(&metrics)
        .expect("upload series");
    let request = crate::start_support::request(rig.root, "expiring.bin", 4 * MIB, "expiring");
    let id =
        crate::start_support::started(service.start_upload(rig.account, request).await).upload_id;
    ticks_past_ttl(&rig.clock);

    // Act
    let expired = service.get_upload(rig.account, id).await;
    let rendered = metrics.render().await.expect("render");

    // Assert
    assert!(
        matches!(expired, Err(UploadError::UploadNotFound)),
        "{expired:?}"
    );
    assert!(rendered.contains("upload_sessions_active 0"), "{rendered}");
    drop(service);
    rig.finish().await;
}
