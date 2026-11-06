//! AC-2 of AZ-3960: `lookup_revisions` splits any input into requests of at most 1,000 hashes and merges the answers.

mod connect_support;
mod request_support;
mod support;
mod tree_support;

use support::{Answer, Route, Script};
use sync_core::{SyncApi, SyncError};
use sync_core_proto::{FileHash, LOOKUP_CHUNK, RevisionRef};
use tree_support::{body_json, connected, file_hash, revision_ref, seen};

/// Hashes `1..=count`; those above `known` are unknown to the server.
fn hashes(count: u64) -> Vec<FileHash> {
    (1..=count).map(file_hash).collect()
}

/// One answer per chunk of `LOOKUP_CHUNK`, each holding the refs of the chunk's known hashes.
fn lookup_script(count: u64, known: u64) -> Script {
    let mut script = Script::new();
    let all: Vec<u64> = (1..=count).collect();
    for chunk in all.chunks(LOOKUP_CHUNK) {
        let refs: Vec<RevisionRef> = chunk
            .iter()
            .filter(|n| **n <= known)
            .map(|n| revision_ref(*n))
            .collect();
        script = script.on(Route::LookupRevisions, Answer::json(200, &refs));
    }
    script
}

fn request_sizes(fixture: &support::Fixture) -> Vec<usize> {
    seen(fixture, Route::LookupRevisions)
        .iter()
        .map(|request| {
            body_json(request)["content_hashes"]
                .as_array()
                .expect("a hash list")
                .len()
        })
        .collect()
}

#[tokio::test]
async fn lookup_of_2510_hashes_sends_1000_1000_and_510_and_merges_2500_refs() {
    // Arrange
    let input = hashes(2510);
    let (api, fixture) = connected(lookup_script(2510, 2500)).await;

    // Act
    let found = api
        .lookup_revisions(&input)
        .await
        .expect("scripted answers");

    // Assert
    assert_eq!(request_sizes(&fixture), vec![1000, 1000, 510]);
    let expected: Vec<RevisionRef> = (1..=2500).map(revision_ref).collect();
    assert_eq!(found, expected);
    assert!(found.iter().all(|found| found.client_meta.is_some()));
}

#[tokio::test]
async fn lookup_chunks_cover_0_1_1000_1001_and_2510_hashes_without_exceeding_1000() {
    for (count, sizes) in [
        (0u64, vec![]),
        (1, vec![1]),
        (1000, vec![1000]),
        (1001, vec![1000, 1]),
        (2510, vec![1000, 1000, 510]),
    ] {
        // Arrange
        let input = hashes(count);
        let (api, fixture) = connected(lookup_script(count, count)).await;

        // Act
        let found = api
            .lookup_revisions(&input)
            .await
            .expect("scripted answers");

        // Assert
        assert_eq!(request_sizes(&fixture), sizes, "{count} hashes");
        assert_eq!(
            found.len(),
            usize::try_from(count).expect("small"),
            "{count} hashes"
        );
    }
}

#[tokio::test]
async fn lookup_sends_the_hashes_as_hex_in_input_order_and_returns_nothing_for_unknown_ones() {
    // Arrange
    let input = hashes(3);
    let (api, fixture) = connected(lookup_script(3, 0)).await;

    // Act
    let found = api
        .lookup_revisions(&input)
        .await
        .expect("scripted answers");

    // Assert
    assert!(found.is_empty());
    let requests = seen(&fixture, Route::LookupRevisions);
    assert_eq!(requests.len(), 1);
    let sent: Vec<String> = body_json(&requests[0])["content_hashes"]
        .as_array()
        .expect("a hash list")
        .iter()
        .map(|hash| hash.as_str().expect("hex text").to_owned())
        .collect();
    let expected: Vec<String> = input.iter().map(ToString::to_string).collect();
    assert_eq!(sent, expected);
}

#[tokio::test]
async fn a_failing_chunk_ends_the_lookup_with_its_error_and_sends_no_further_chunk() {
    // Arrange
    let script = Script::new()
        .on(
            Route::LookupRevisions,
            Answer::json(200, &Vec::<RevisionRef>::new()),
        )
        .on(
            Route::LookupRevisions,
            Answer::problem(sc_error::ErrorCode::BatchTooLarge),
        );
    let (api, fixture) = connected(script).await;

    // Act
    let result = api.lookup_revisions(&hashes(2510)).await;

    // Assert
    assert_eq!(result, Err(SyncError::BatchTooLarge));
    assert_eq!(request_sizes(&fixture), vec![1000, 1000]);
}
