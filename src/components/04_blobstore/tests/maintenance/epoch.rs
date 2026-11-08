//! AC-2: the `.epoch` marker is read and written only through the port, and a write is atomic.

use std::fs;

use contracts::EpochMarker;
use sunnycloud_blobstore::FileEpochMarker;

use crate::support::Rig;

fn marker(rig: &Rig) -> FileEpochMarker {
    FileEpochMarker::new(&rig.config).expect("marker")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_volume_has_no_marker_and_a_written_value_is_read_back() {
    // Arrange
    let rig = Rig::new().await;
    let marker = marker(&rig);

    // Act
    let fresh = marker.read().await.expect("read fresh");
    marker.write(7).await.expect("write 7");
    let first = marker.read().await.expect("read 7");
    marker.write(8).await.expect("write 8");
    let second = marker.read().await.expect("read 8");

    // Assert
    assert_eq!(fresh, None);
    assert_eq!(first, Some(7));
    assert_eq!(second, Some(8));
    assert_eq!(
        fs::read(rig.volume.join(".epoch")).expect("marker file"),
        b"8\n"
    );
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_temp_file_left_by_a_killed_write_is_ignored_and_replaced_by_the_next_write() {
    // Arrange
    let rig = Rig::new().await;
    let marker = marker(&rig);
    marker.write(7).await.expect("write 7");
    fs::write(rig.volume.join(".epoch.tmp"), b"9").expect("cut temp file");

    // Act
    let after_kill = marker.read().await.expect("read after the kill");
    marker.write(9).await.expect("write 9");
    let after_write = marker.read().await.expect("read after the write");

    // Assert
    assert_eq!(after_kill, Some(7));
    assert_eq!(after_write, Some(9));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_marker_file_that_is_cut_or_not_a_number_is_an_error_never_a_value() {
    // Arrange
    let rig = Rig::new().await;
    let marker = marker(&rig);
    let mut results = Vec::new();

    // Act
    for content in [&b""[..], b"12", b"1x\n", b"\xff\n"] {
        fs::write(rig.volume.join(".epoch"), content).expect("marker file");
        results.push(marker.read().await);
    }

    // Assert
    assert!(
        results.iter().all(Result::is_err),
        "no partial content is a value: {results:?}"
    );
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_volume_that_is_not_there_is_an_error_not_a_fresh_volume() {
    // Arrange
    let rig = Rig::new().await;
    let mut config = rig.config.clone();
    config.blob_root = Some(
        rig.volume
            .join("not-mounted")
            .to_string_lossy()
            .into_owned(),
    );
    let marker = FileEpochMarker::new(&config).expect("marker");

    // Act
    let read = marker.read().await;
    let write = marker.write(1).await;

    // Assert
    assert!(read.is_err());
    assert!(write.is_err());
    rig.finish().await;
}
