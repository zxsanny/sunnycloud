//! AC-3 to AC-5 of AZ-3960: delete carries the base revision, move never does, tree errors are typed.

mod connect_support;
mod request_support;
mod support;
mod tree_support;

use sc_error::ErrorCode;
use support::{Answer, Fault, Route, Script};
use sync_core::{SyncApi, SyncError};
use sync_core_proto::{NodeKind, NodeView};
use tree_support::{body_json, connected, id, seen};

fn folder(node: u128, parent: u128, name: &str) -> NodeView {
    NodeView {
        node_id: id(node),
        parent_id: Some(id(parent)),
        kind: NodeKind::Folder,
        name: name.to_owned(),
        current_revision: None,
    }
}

#[tokio::test]
async fn delete_of_a_file_sends_the_base_revision_and_reports_edited_since() {
    // Arrange
    let (rev1, file) = (id(0xA1), id(0xF1));
    let script = Script::new().on(Route::DeleteNode, Answer::problem(ErrorCode::EditedSince));
    let (api, fixture) = connected(script).await;

    // Act
    let result = api.delete_node(file, Some(rev1)).await;

    // Assert
    assert_eq!(result, Err(SyncError::EditedSince { current: None }));
    let requests = seen(&fixture, Route::DeleteNode);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "DELETE");
    assert_eq!(
        requests[0].target,
        format!("/api/v1/nodes/{file}?base_revision={rev1}")
    );
}

#[tokio::test]
async fn delete_succeeds_on_204_and_a_folder_delete_carries_no_base_revision() {
    // Arrange
    let script = Script::new()
        .on(Route::DeleteNode, Answer::no_content())
        .on(Route::DeleteNode, Answer::no_content());
    let (api, fixture) = connected(script).await;
    let (file, directory) = (id(0xF1), id(0xD1));

    // Act
    let file_result = api.delete_node(file, Some(id(0xA2))).await;
    let folder_result = api.delete_node(directory, None).await;

    // Assert
    assert_eq!(file_result, Ok(()));
    assert_eq!(folder_result, Ok(()));
    let targets: Vec<String> = seen(&fixture, Route::DeleteNode)
        .into_iter()
        .map(|request| request.target)
        .collect();
    assert_eq!(
        targets,
        vec![
            format!("/api/v1/nodes/{file}?base_revision={}", id(0xA2)),
            format!("/api/v1/nodes/{directory}"),
        ]
    );
}

#[tokio::test]
async fn move_sends_no_base_revision_and_the_node_id_is_unchanged_in_the_result() {
    // Arrange
    let (moved, new_parent) = (id(0xD1), id(0xD2));
    let answer = folder(0xD1, 0xD2, "renamed");
    let (api, fixture) =
        connected(Script::new().on(Route::UpdateNode, Answer::json(200, &answer))).await;

    // Act
    let result = api.move_node(moved, new_parent, "renamed").await;

    // Assert
    assert_eq!(result, Ok(answer));
    let requests = seen(&fixture, Route::UpdateNode);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "PATCH");
    assert_eq!(requests[0].target, format!("/api/v1/nodes/{moved}"));
    assert_eq!(requests[0].header("content-type"), Some("application/json"));
    let body = body_json(&requests[0]);
    assert_eq!(
        body,
        serde_json::json!({ "parent_id": new_parent, "name": "renamed" })
    );
    assert!(body.get("base_revision").is_none());
}

#[tokio::test]
async fn create_folder_posts_parent_and_name_and_returns_the_new_node() {
    // Arrange
    let answer = folder(0xC1, 0xD2, "Photos \u{202E}x");
    let (api, fixture) =
        connected(Script::new().on(Route::CreateNode, Answer::json(201, &answer))).await;

    // Act
    let result = api.create_folder(id(0xD2), "Photos \u{202E}x").await;

    // Assert
    assert_eq!(result, Ok(answer));
    let requests = seen(&fixture, Route::CreateNode);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].target, "/api/v1/nodes");
    assert_eq!(
        body_json(&requests[0]),
        serde_json::json!({ "parent_id": id(0xD2), "name": "Photos \u{202E}x" })
    );
}

#[tokio::test]
async fn each_scripted_tree_code_becomes_its_typed_error_for_create_and_move() {
    let table = [
        (ErrorCode::NameConflict, SyncError::NameConflict),
        (ErrorCode::InvalidName, SyncError::InvalidName),
        (ErrorCode::Cycle, SyncError::Cycle),
        (ErrorCode::ParentNotFound, SyncError::ParentNotFound),
        (ErrorCode::ParentDeleted, SyncError::ParentGone),
        (ErrorCode::NotAFolder, SyncError::NotAFolder),
    ];
    for (code, expected) in table {
        // Arrange
        let script = Script::new()
            .always(Route::CreateNode, Answer::problem(code))
            .always(Route::UpdateNode, Answer::problem(code));
        let (api, _fixture) = connected(script).await;

        // Act
        let created = api.create_folder(id(1), "docs").await;
        let moved = api.move_node(id(2), id(1), "docs").await;

        // Assert
        assert_eq!(created, Err(expected.clone()), "create with {code:?}");
        assert_eq!(moved, Err(expected), "move with {code:?}");
    }
}

#[tokio::test]
async fn create_folder_is_sent_once_after_a_lost_answer_and_move_is_sent_again() {
    // Arrange
    let answer = folder(0xD1, 0xD2, "renamed");
    let script = Script::new()
        .on(
            Route::CreateNode,
            Answer::status(204).fault(Fault::DropConnection),
        )
        .on(
            Route::UpdateNode,
            Answer::status(204).fault(Fault::DropConnection),
        )
        .on(Route::UpdateNode, Answer::json(200, &answer));
    let (api, fixture) = connected(script).await;

    // Act
    let created = api.create_folder(id(0xD2), "renamed").await;
    let moved = api.move_node(id(0xD1), id(0xD2), "renamed").await;

    // Assert
    assert!(
        matches!(created, Err(SyncError::Transport(_))),
        "{created:?}"
    );
    assert_eq!(seen(&fixture, Route::CreateNode).len(), 1);
    assert_eq!(moved, Ok(answer));
    assert_eq!(seen(&fixture, Route::UpdateNode).len(), 2);
}

#[tokio::test]
async fn an_upload_planner_code_on_a_tree_route_is_a_protocol_violation() {
    // Arrange
    let script = Script::new().on(Route::CreateNode, Answer::problem(ErrorCode::BlockConflict));
    let (api, _fixture) = connected(script).await;

    // Act
    let result = api.create_folder(id(1), "docs").await;

    // Assert
    assert_eq!(
        result,
        Err(SyncError::ProtocolViolation {
            detail: "upload answer on a tree route"
        })
    );
}
