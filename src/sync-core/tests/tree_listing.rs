//! AC-1 and AC-6 of AZ-3960: paged listings with content hashes, hostile names returned as data, root and revision reads.

mod connect_support;
mod request_support;
mod support;
mod tree_support;

use serde_json::json;
use support::{Answer, Route, Script};
use sync_core::{SyncApi, SyncError};
use sync_core_proto::{ListingItem, ListingPage, NodeKind, NodeView, PageToken, RevisionView};
use tree_support::{connected, file_hash, file_item, folder_item, id, seen};

const PAGE: usize = 1000;
const ENTRIES: u64 = 2500;

fn entry(n: u64) -> ListingItem {
    if n.is_multiple_of(5) {
        folder_item(n, &format!("folder-{n}"))
    } else {
        file_item(n, &format!("file-{n}.bin"))
    }
}

fn listing_script(tokens: &[&str]) -> Script {
    let items: Vec<ListingItem> = (1..=ENTRIES).map(entry).collect();
    let mut script = Script::new();
    for (index, chunk) in items.chunks(PAGE).enumerate() {
        let next = tokens
            .get(index)
            .map(|token| PageToken((*token).to_owned()));
        let page = ListingPage {
            items: chunk.to_vec(),
            next,
        };
        script = script.on(Route::ListChildren, Answer::json(200, &page));
    }
    script
}

#[tokio::test]
async fn list_children_chains_2500_entries_over_three_pages_with_hashes_on_files_only() {
    // Arrange
    let (api, fixture) = connected(listing_script(&["t+k/1=", "page 2"])).await;
    let folder = id(1);

    // Act
    let mut items = Vec::new();
    let mut after: Option<PageToken> = None;
    let mut pages = 0;
    loop {
        let page = api
            .list_children(folder, after.as_ref())
            .await
            .expect("a scripted page");
        pages += 1;
        items.extend(page.items);
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }

    // Assert
    assert_eq!(pages, 3);
    assert_eq!(items.len(), 2500);
    for (index, item) in items.iter().enumerate() {
        let n = u64::try_from(index).expect("small index") + 1;
        assert_eq!(item.node_id, id(u128::from(n)));
        match item.kind {
            NodeKind::File => assert_eq!(item.content_hash, Some(file_hash(n)), "file {n}"),
            NodeKind::Folder => assert_eq!(item.content_hash, None, "folder {n}"),
        }
    }
    let targets: Vec<String> = seen(&fixture, Route::ListChildren)
        .into_iter()
        .map(|request| request.target)
        .collect();
    let base = format!("/api/v1/nodes/{folder}/children");
    assert_eq!(
        targets,
        vec![
            base.clone(),
            format!("{base}?after=t%2Bk%2F1%3D"),
            format!("{base}?after=page%202"),
        ]
    );
}

#[tokio::test]
async fn list_children_returns_hostile_names_byte_for_byte() {
    // Arrange
    let long = "n".repeat(8 * 1024);
    let names = [
        "../../etc/passwd".to_owned(),
        "report\u{202E}gpj.exe".to_owned(),
        long,
    ];
    let page = ListingPage {
        items: names
            .iter()
            .enumerate()
            .map(|(index, name)| file_item(index as u64 + 1, name))
            .collect(),
        next: None,
    };
    let (api, _fixture) =
        connected(Script::new().on(Route::ListChildren, Answer::json(200, &page))).await;

    // Act
    let listed = api
        .list_children(id(1), None)
        .await
        .expect("a scripted page");

    // Assert
    let returned: Vec<&str> = listed.items.iter().map(|item| item.name.as_str()).collect();
    let expected: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(returned, expected);
    assert_eq!(listed.items[2].name.len(), 8 * 1024);
}

#[tokio::test]
async fn list_children_of_a_file_is_not_a_folder_and_of_an_unknown_node_is_not_found() {
    // Arrange
    let script = Script::new()
        .on(
            Route::ListChildren,
            Answer::problem(sc_error::ErrorCode::NotAFolder),
        )
        .on(
            Route::ListChildren,
            Answer::problem(sc_error::ErrorCode::NotFound),
        );
    let (api, _fixture) = connected(script).await;

    // Act
    let file = api.list_children(id(7), None).await;
    let unknown = api.list_children(id(8), None).await;

    // Assert
    assert_eq!(file, Err(SyncError::NotAFolder));
    assert_eq!(unknown, Err(SyncError::NotFound));
}

#[tokio::test]
async fn root_node_and_get_revision_return_the_scripted_views() {
    // Arrange
    let root = NodeView {
        node_id: id(1),
        parent_id: None,
        kind: NodeKind::Folder,
        name: String::new(),
        current_revision: None,
    };
    let revision = RevisionView {
        revision_id: id(0x1001),
        blocklist: vec![sc_ids_hash(1), sc_ids_hash(2)],
        content_hash: file_hash(9),
        size: 4_194_304,
        client_meta: Some(json!({ "device": "pc" })),
    };
    let script = Script::new()
        .on(Route::RootNode, Answer::json(200, &root))
        .on(Route::GetRevision, Answer::json(200, &revision))
        .on(
            Route::GetRevision,
            Answer::problem(sc_error::ErrorCode::NotFound),
        );
    let (api, fixture) = connected(script).await;

    // Act
    let got_root = api.root_node().await;
    let got_revision = api.get_revision(revision.revision_id).await;
    let missing = api.get_revision(id(0x9999)).await;

    // Assert
    assert_eq!(got_root, Ok(root));
    assert_eq!(got_revision, Ok(revision.clone()));
    assert_eq!(missing, Err(SyncError::NotFound));
    let targets: Vec<String> = seen(&fixture, Route::GetRevision)
        .into_iter()
        .map(|request| request.target)
        .collect();
    assert_eq!(
        targets,
        vec![
            format!("/api/v1/revisions/{}", revision.revision_id),
            format!("/api/v1/revisions/{}", id(0x9999)),
        ]
    );
}

#[tokio::test]
async fn a_body_that_is_not_the_dto_is_a_protocol_violation() {
    // Arrange
    let script = Script::new().on(
        Route::RootNode,
        Answer::json(200, &json!({ "unexpected": true })),
    );
    let (api, _fixture) = connected(script).await;

    // Act
    let result = api.root_node().await;

    // Assert
    assert_eq!(
        result,
        Err(SyncError::ProtocolViolation {
            detail: "malformed response body"
        })
    );
}

fn sc_ids_hash(n: u8) -> sync_core_proto::Hash32 {
    sync_core_proto::Hash32::from([n; 32])
}
