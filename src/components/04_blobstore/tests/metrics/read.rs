//! AC-3: `blob_read_duration_seconds` holds one observation per successful `open`, by `kind`.

use contracts::{BlobReader, ByteRange};
use uuid::Uuid;

use crate::support::Rig;
use crate::{registry, sample};

const RANGE: &str = "sunnycloud_blob_read_duration_seconds_count{kind=\"range\"}";
const STREAM: &str = "sunnycloud_blob_read_duration_seconds_count{kind=\"stream\"}";

#[tokio::test(flavor = "multi_thread")]
async fn ten_opens_add_ten_observations_split_by_range_and_whole_blob_and_a_failed_open_adds_none()
{
    // Arrange
    let mut rig = Rig::new().await;
    let (metrics, blob) = registry();
    rig.reader = rig.reader.with_metrics(blob);
    let stored = rig.store(64 * 1024, 5).await;
    let range = ByteRange {
        start: 0,
        end: 4095,
    };

    // Act
    let mut streams = Vec::new();
    for open in 0..10 {
        let range = (open % 2 == 0).then_some(range);
        let stream = rig
            .reader
            .open(
                stored.blob_id,
                &stored.wrapped_key,
                stored.master_key_id,
                stored.size,
                range,
            )
            .await
            .expect("open");
        streams.push(stream);
    }
    let missing = rig
        .reader
        .open(
            Uuid::new_v4(),
            &stored.wrapped_key,
            stored.master_key_id,
            stored.size,
            None,
        )
        .await;
    let scrape = metrics.render().await.expect("render");

    // Assert
    assert!(missing.is_err());
    assert_eq!(sample(&scrape, RANGE), Some(5.0), "{scrape}");
    assert_eq!(sample(&scrape, STREAM), Some(5.0), "{scrape}");
    rig.finish().await;
}
