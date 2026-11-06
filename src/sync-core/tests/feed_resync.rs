//! AC-3 of AZ-3965: 410 `resync_required` and 422 `invalid_cursor` become `ResyncRequired`, and the host-side
//! resync reads the head cursor before the first listing. A resync answer is not a failure to back off from.

mod connect_support;
mod feed_support;
mod request_support;
mod support;
mod tree_support;

use std::time::Instant;

use feed_support::{
    connected_with, empty_listing, head_answer, item, ms, page, page_answer, resync, root_answer,
};
use sc_error::ErrorCode;
use serde_json::json;
use support::{Answer, Route, Script};
use sync_core::{SyncApi, SyncError};
use sync_core_proto::{Cursor, ResyncReason};

const STALE: Cursor = Cursor::new(1, 100);
const NEW_HEAD: Cursor = Cursor::new(2, 0);

fn epoch_changed() -> Answer {
    Answer::problem(ErrorCode::ResyncRequired).extra("reason", json!("epoch_changed"))
}

fn restored_server() -> Script {
    Script::new()
        .on(Route::Changes, epoch_changed())
        .on(
            Route::Changes,
            page_answer(&page(vec![item(1, 9)], Cursor::new(2, 1), false)),
        )
        .on(Route::ChangesHead, head_answer(NEW_HEAD))
        .on(Route::RootNode, root_answer())
        .on(Route::ListChildren, empty_listing())
}

#[tokio::test]
async fn a_410_with_epoch_changed_and_a_422_invalid_cursor_are_resync_required() {
    // Arrange
    let (api, _fixture) = connected_with(
        Script::new()
            .on(Route::Changes, epoch_changed())
            .on(Route::Changes, Answer::problem(ErrorCode::InvalidCursor)),
        30,
        |config| config,
    )
    .await;

    // Act
    let epoch = api.poll_changes(STALE, ms(0), None).await;
    let invalid = api.poll_changes(STALE, ms(0), None).await;

    // Assert
    assert_eq!(
        epoch,
        Err(SyncError::ResyncRequired {
            reason: ResyncReason::EpochChanged
        })
    );
    assert_eq!(
        invalid,
        Err(SyncError::ResyncRequired {
            reason: ResyncReason::InvalidCursor
        })
    );
}

#[tokio::test]
async fn the_resync_reads_the_head_cursor_before_the_first_listing_and_polls_on_from_it_at_once() {
    // Arrange
    let (api, fixture) = connected_with(restored_server(), 30, |config| sync_core::SyncConfig {
        retry_base: ms(600),
        retry_max: ms(2000),
        ..config
    })
    .await;
    let refused = api.poll_changes(STALE, ms(0), None).await;
    assert!(matches!(refused, Err(SyncError::ResyncRequired { .. })));

    // Act
    let (head, listing) = resync(&api).await;
    let started = Instant::now();
    let polled = api.poll_changes(head, ms(0), None).await;
    let elapsed = started.elapsed();

    // Assert
    assert_eq!(head, NEW_HEAD);
    assert!(listing.items.is_empty());
    assert!(polled.is_ok(), "{polled:?}");
    assert!(
        elapsed < ms(400),
        "the poll after a resync waited {elapsed:?}"
    );
    let routes: Vec<Route> = fixture
        .requests()
        .into_iter()
        .filter_map(|request| request.route)
        .filter(|route| *route != Route::ServerInfo)
        .collect();
    assert_eq!(
        routes,
        vec![
            Route::Changes,
            Route::ChangesHead,
            Route::RootNode,
            Route::ListChildren,
            Route::Changes
        ]
    );
}
