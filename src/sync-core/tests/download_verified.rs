//! `download` (AZ-3964): only blocks that match the blocklist reach the sink, the whole file matches `content_hash`,
//! resume starts at a block boundary, missing content is typed.

mod connect_support;
mod download_support;
mod request_support;
mod support;
mod tree_support;

use std::sync::Arc;

use bytes::Bytes;
use download_support::{
    BLOCK, SpySink, content_script, range_start, ranges, reference, revision_script, view_of,
};
use sc_error::ErrorCode;
use support::{Answer, Route, Script, seeded_bytes};
use sync_core::{DownloadSink, SyncApi, SyncError, Verified};
use tree_support::{connected, seen};

const BLOCKS: usize = 20;

fn forty_mib() -> Bytes {
    seeded_bytes(40, BLOCKS * BLOCK)
}

#[tokio::test]
async fn a_bit_flipped_in_block_7_ends_with_hash_mismatch_and_blocks_0_to_6_only() {
    // Arrange
    let data = forty_mib();
    let view = view_of(&data);
    let served = data.clone();
    let script = content_script(&view, move |_| {
        Answer::content(served.clone()).flip_bit(7 * BLOCK + 5, 3)
    });
    let (api, _fixture) = connected(script).await;
    let sink = SpySink::new();

    // Act
    let result = api.download(&reference(&view), sink.clone(), 0).await;

    // Assert
    assert_eq!(result, Err(SyncError::HashMismatch { block: 7 }));
    assert_eq!(sink.offsets(), SpySink::block_offsets(0, 7));
    assert_eq!(sink.content(), data[..7 * BLOCK]);
    assert_eq!(sink.finishes(), vec![false]);
}

#[tokio::test]
async fn resuming_at_block_7_requests_from_block_7_and_returns_the_content_hash() {
    // Arrange
    let data = forty_mib();
    let view = view_of(&data);
    let served = data.clone();
    let (api, fixture) = connected(content_script(&view, move |_| {
        Answer::content(served.clone())
    }))
    .await;
    let sink = SpySink::new();

    // Act
    let result = api
        .download(&reference(&view), sink.clone(), (7 * BLOCK) as u64)
        .await;

    // Assert
    assert_eq!(
        result,
        Ok(Verified {
            file_hash: view.content_hash
        })
    );
    assert_eq!(range_start(&ranges(&fixture)[0]), (7 * BLOCK) as u64);
    assert_eq!(sink.offsets(), SpySink::block_offsets(7, BLOCKS));
    assert_eq!(sink.finishes(), vec![true]);
}

#[tokio::test]
async fn a_resume_offset_inside_a_block_restarts_that_block() {
    // Arrange
    let data = seeded_bytes(41, 3 * BLOCK);
    let view = view_of(&data);
    let served = data.clone();
    let (api, fixture) = connected(content_script(&view, move |_| {
        Answer::content(served.clone())
    }))
    .await;
    let sink = SpySink::new();

    // Act
    let result = api
        .download(&reference(&view), sink.clone(), (BLOCK + 12_345) as u64)
        .await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(range_start(&ranges(&fixture)[0]), BLOCK as u64);
    assert_eq!(sink.offsets(), SpySink::block_offsets(1, 3));
}

#[tokio::test]
async fn files_of_every_shape_arrive_whole_and_verified() {
    for (seed, len) in [
        (1, 0),
        (2, 1),
        (3, BLOCK - 1),
        (4, BLOCK),
        (5, BLOCK + 1),
        (6, 3 * BLOCK + 17),
    ] {
        // Arrange
        let data = seeded_bytes(seed, len);
        let view = view_of(&data);
        let served = data.clone();
        let (api, fixture) = connected(content_script(&view, move |_| {
            Answer::content(served.clone())
        }))
        .await;
        let sink = SpySink::new();

        // Act
        let result = api.download(&reference(&view), sink.clone(), 0).await;

        // Assert
        assert_eq!(
            result,
            Ok(Verified {
                file_hash: view.content_hash
            }),
            "length {len}"
        );
        assert_eq!(sink.content(), data.to_vec(), "length {len}");
        assert_eq!(sink.finishes(), vec![true], "length {len}");
        assert_eq!(
            seen(&fixture, Route::RevisionContent).len(),
            len.div_ceil(BLOCK),
            "one request per block, none for an empty file"
        );
    }
}

#[tokio::test]
async fn resuming_at_the_end_of_a_block_aligned_file_fetches_nothing() {
    // Arrange
    let data = seeded_bytes(8, 2 * BLOCK);
    let view = view_of(&data);
    let (api, fixture) = connected(revision_script(&view)).await;
    let sink = SpySink::new();

    // Act
    let result = api
        .download(&reference(&view), sink.clone(), data.len() as u64)
        .await;

    // Assert
    assert_eq!(
        result,
        Ok(Verified {
            file_hash: view.content_hash
        })
    );
    assert!(seen(&fixture, Route::RevisionContent).is_empty());
    assert_eq!(sink.finishes(), vec![true]);
}

#[tokio::test]
async fn a_resume_offset_beyond_the_file_is_refused_before_any_content_request() {
    // Arrange
    let data = seeded_bytes(9, BLOCK + 5);
    let view = view_of(&data);
    let (api, fixture) = connected(revision_script(&view)).await;
    let sink = SpySink::new();

    // Act
    let result = api
        .download(&reference(&view), sink.clone(), data.len() as u64 + 1)
        .await;

    // Assert
    assert!(
        matches!(result, Err(SyncError::ProtocolViolation { .. })),
        "{result:?}"
    );
    assert!(seen(&fixture, Route::RevisionContent).is_empty());
    assert!(sink.offsets().is_empty());
}

#[tokio::test]
async fn a_missing_revision_is_not_found_and_the_sink_is_untouched() {
    // Arrange
    let data = seeded_bytes(10, 10);
    let view = view_of(&data);
    let script = Script::new().always(Route::GetRevision, Answer::problem(ErrorCode::NotFound));
    let (api, _fixture) = connected(script).await;
    let sink = SpySink::new();

    // Act
    let result = api.download(&reference(&view), sink.clone(), 0).await;

    // Assert
    assert_eq!(result, Err(SyncError::NotFound));
    assert!(sink.offsets().is_empty());
    assert!(sink.finishes().is_empty());
}

#[tokio::test]
async fn content_answers_404_become_not_found_and_blob_unavailable() {
    for (code, expected) in [
        (ErrorCode::NotFound, SyncError::NotFound),
        (ErrorCode::ContentUnavailable, SyncError::BlobUnavailable),
    ] {
        // Arrange
        let data = seeded_bytes(11, 2 * BLOCK);
        let view = view_of(&data);
        let (api, _fixture) =
            connected(content_script(&view, move |_| Answer::problem(code))).await;
        let sink = SpySink::new();

        // Act
        let result = api.download(&reference(&view), sink.clone(), 0).await;

        // Assert
        assert_eq!(result, Err(expected));
        assert!(sink.offsets().is_empty());
        assert_eq!(sink.finishes(), vec![false]);
    }
}

#[tokio::test]
async fn a_blocklist_that_does_not_roll_up_to_the_content_hash_is_not_verified() {
    // Arrange
    let data = seeded_bytes(12, 2 * BLOCK);
    let mut view = view_of(&data);
    view.content_hash = view_of(b"another file").content_hash;
    let served = data.clone();
    let (api, _fixture) = connected(content_script(&view, move |_| {
        Answer::content(served.clone())
    }))
    .await;
    let sink = SpySink::new();
    let mut revision = reference(&view);
    revision.content_hash = view.content_hash;

    // Act
    let result = api.download(&revision, sink.clone(), 0).await;

    // Assert
    assert!(
        matches!(result, Err(SyncError::ProtocolViolation { .. })),
        "{result:?}"
    );
    assert_eq!(sink.finishes(), vec![false]);
}

#[tokio::test]
async fn a_reference_with_another_content_hash_than_the_revision_is_refused() {
    // Arrange
    let data = seeded_bytes(13, 10);
    let view = view_of(&data);
    let mut revision = reference(&view);
    revision.content_hash = view_of(b"stale").content_hash;
    let (api, fixture) = connected(revision_script(&view)).await;
    let sink = SpySink::new();

    // Act
    let result = api.download(&revision, sink.clone(), 0).await;

    // Assert
    assert!(
        matches!(result, Err(SyncError::ProtocolViolation { .. })),
        "{result:?}"
    );
    assert!(seen(&fixture, Route::RevisionContent).is_empty());
}

#[tokio::test]
async fn a_server_that_ignores_range_is_not_accepted() {
    // Arrange
    let data = seeded_bytes(14, 2 * BLOCK);
    let view = view_of(&data);
    let whole = data.clone();
    let script = content_script(&view, move |_| {
        Answer::bytes(200, "application/octet-stream", whole.clone())
    });
    let (api, _fixture) = connected(script).await;
    let sink = SpySink::new();

    // Act
    let result = api.download(&reference(&view), sink.clone(), 0).await;

    // Assert
    assert!(
        matches!(result, Err(SyncError::ProtocolViolation { .. })),
        "{result:?}"
    );
    assert!(sink.offsets().is_empty());
}

#[tokio::test]
async fn an_answer_for_another_range_is_not_accepted() {
    // Arrange
    let data = seeded_bytes(15, 2 * BLOCK);
    let view = view_of(&data);
    let first_block = data.slice(..BLOCK);
    let script = content_script(&view, move |_| {
        Answer::bytes(206, "application/octet-stream", first_block.clone()).header(
            "Content-Range",
            &format!("bytes 5-{}/{}", BLOCK + 4, 2 * BLOCK),
        )
    });
    let (api, _fixture) = connected(script).await;
    let sink = SpySink::new();

    // Act
    let result = api.download(&reference(&view), sink.clone(), 0).await;

    // Assert
    assert!(
        matches!(result, Err(SyncError::ProtocolViolation { .. })),
        "{result:?}"
    );
    assert!(sink.offsets().is_empty());
}

#[tokio::test]
async fn a_clean_answer_shorter_than_the_range_is_not_complete() {
    // Arrange
    let data = seeded_bytes(16, 2 * BLOCK);
    let view = view_of(&data);
    let short = data.slice(..BLOCK - 10);
    let script = content_script(&view, move |_| {
        Answer::bytes(206, "application/octet-stream", short.clone()).header(
            "Content-Range",
            &format!("bytes 0-{}/{}", BLOCK - 1, 2 * BLOCK),
        )
    });
    let (api, _fixture) = connected(script).await;
    let sink = SpySink::new();

    // Act
    let result = api.download(&reference(&view), sink.clone(), 0).await;

    // Assert
    assert!(
        matches!(result, Err(SyncError::ProtocolViolation { .. })),
        "{result:?}"
    );
    assert!(sink.offsets().is_empty());
    assert_eq!(sink.finishes(), vec![false]);
}

#[tokio::test]
async fn a_failing_sink_write_ends_with_io_and_no_later_block() {
    // Arrange
    let data = seeded_bytes(17, 3 * BLOCK);
    let view = view_of(&data);
    let served = data.clone();
    let (api, _fixture) = connected(content_script(&view, move |_| {
        Answer::content(served.clone())
    }))
    .await;
    let sink = SpySink::failing_at(BLOCK as u64);
    let as_sink: Arc<dyn DownloadSink> = sink.clone();

    // Act
    let result = api.download(&reference(&view), as_sink, 0).await;

    // Assert
    assert_eq!(
        result,
        Err(SyncError::Io {
            kind: std::io::ErrorKind::PermissionDenied
        })
    );
    assert_eq!(sink.offsets(), SpySink::block_offsets(0, 1));
    assert_eq!(sink.finishes(), vec![false]);
}
