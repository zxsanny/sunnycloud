use contracts::{
    ListingPage, NodeView, RevisionMatch, RevisionRef, RevisionView, TreeReaderError,
    TreeServiceError,
};
use serde_json::json;

use super::support::*;
use crate::auth::support::{DEVICE_ACCOUNT, MEMBER_ACCOUNT};

const GET_NODE: &str = "TreeReader::get_node";
const LIST: &str = "TreeReader::list_children";
const GET_REVISION: &str = "TreeReader::get_revision";
const LOOKUP: &str = "TreeReader::lookup_revisions";
const CREATE: &str = "TreeService::create_folder";
const UPDATE: &str = "TreeService::update_node";
const DELETE: &str = "TreeService::delete_node";
const FIND: &str = "TreeService::find_revisions";

fn node_ok(node: NodeView) -> Result<NodeView, TreeReaderError> {
    Ok(node)
}

#[tokio::test]
async fn every_tree_operation_makes_one_port_call_for_the_account_of_the_session() {
    // Arrange
    let stack = Stack::start().await;
    let script = &stack.script;
    script.push(GET_NODE, node_ok(folder_node(FOLDER, None)));
    script.push(GET_NODE, node_ok(file_node(NODE)));
    script.push(
        CREATE,
        Ok::<_, TreeServiceError>(folder_node(COPY, Some(FOLDER))),
    );
    script.push(UPDATE, Ok::<_, TreeServiceError>(file_node(NODE)));
    script.push(DELETE, Ok::<(), TreeServiceError>(()));
    script.push(GET_REVISION, Ok::<_, TreeReaderError>(revision_view(None)));
    script.push(
        FIND,
        Ok::<_, TreeServiceError>(vec![RevisionRef {
            revision_id: REVISION,
            node_id: NODE,
        }]),
    );
    script.push(
        LOOKUP,
        Ok::<_, TreeReaderError>(vec![revision_match(Some(r#"{"device":"laptop"}"#))]),
    );
    let hash = hex(0xAB);

    // Act
    let root = stack.as_member("GET", "/api/v1/nodes/root", "").await;
    let get = stack
        .as_member("GET", &format!("/api/v1/nodes/{NODE}"), "")
        .await;
    let create = stack
        .as_member(
            "POST",
            "/api/v1/nodes",
            &json!({"parent_id": FOLDER, "name": "Docs"}).to_string(),
        )
        .await;
    let update = stack
        .as_member(
            "PATCH",
            &format!("/api/v1/nodes/{NODE}"),
            &json!({"name": "new.pdf", "parent_id": FOLDER}).to_string(),
        )
        .await;
    let delete = stack
        .as_member(
            "DELETE",
            &format!("/api/v1/nodes/{NODE}?base_revision={BASE}"),
            "",
        )
        .await;
    let revision = stack
        .as_member("GET", &format!("/api/v1/revisions/{REVISION}"), "")
        .await;
    let find = stack
        .as_member(
            "GET",
            "/api/v1/revisions?client_device=laptop&offset=5&limit=10",
            "",
        )
        .await;
    let lookup = stack
        .as_member(
            "POST",
            "/api/v1/revisions:lookup",
            &json!({"content_hashes": [hash]}).to_string(),
        )
        .await;

    // Assert
    let statuses = [
        &root, &get, &create, &update, &delete, &revision, &find, &lookup,
    ]
    .map(|r| r.status);
    assert_eq!(statuses, [200, 200, 201, 200, 204, 200, 200, 200]);
    assert_eq!(root.json()["kind"], "folder");
    assert_eq!(root.json()["parent_id"], json!(null));
    assert_eq!(get.json()["node_id"], NODE.to_string());
    assert_eq!(get.json()["current_revision"], REVISION.to_string());
    assert_eq!(get.json()["content_hash"], hex(0xAB));
    assert_eq!(get.json()["client_mtime"], CREATED_TEXT);
    assert_eq!(create.json()["parent_id"], FOLDER.to_string());
    assert!(delete.body.is_empty());
    assert_eq!(revision.json()["blocklist"], json!([hex(1), hex(2)]));
    assert_eq!(revision.json()["client_meta"], json!(null));
    assert_eq!(
        find.json()["items"],
        json!([{"revision_id": REVISION.to_string(), "node_id": NODE.to_string()}])
    );
    assert_eq!(
        lookup.json(),
        json!([{
            "revision_id": REVISION.to_string(),
            "node_id": NODE.to_string(),
            "content_hash": hex(0xAB),
            "client_meta": {"device": "laptop"}
        }])
    );
    let account = MEMBER_ACCOUNT;
    let page = "Page { offset: 5, limit: 10 }";
    assert_eq!(
        stack.script.log(),
        [
            format!("{GET_NODE} ({account}, Root)"),
            format!("{GET_NODE} ({account}, Id({NODE}))"),
            format!("{CREATE} ({account}, {FOLDER}, \"Docs\")"),
            format!("{UPDATE} ({account}, {NODE}, Some(\"new.pdf\"), Some({FOLDER}))"),
            format!("{DELETE} ({account}, {NODE}, Some({BASE}))"),
            format!("{GET_REVISION} ({account}, {REVISION})"),
            format!("{FIND} ({account}, Some(\"laptop\"), {page})"),
            format!("{LOOKUP} ({account}, [{:?}])", file_hash(0xAB)),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_device_token_reaches_the_tree_with_its_own_account() {
    // Arrange
    let stack = Stack::start().await;
    stack
        .script
        .push(GET_NODE, node_ok(folder_node(FOLDER, None)));

    // Act
    let root = stack.as_device("GET", "/api/v1/nodes/root", "").await;

    // Assert
    assert_eq!(root.status, 200);
    assert_eq!(
        stack.log(GET_NODE),
        [format!("{GET_NODE} ({DEVICE_ACCOUNT}, Root)")]
    );
    stack.stop().await;
}

#[tokio::test]
async fn ac7_the_listing_passes_the_folder_the_key_and_the_limit_and_answers_the_items_and_the_next_key()
 {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        LIST,
        Ok::<_, TreeReaderError>(ListingPage {
            items: vec![file_node(NODE), folder_node(COPY, Some(FOLDER))],
            next: Some("next key/é".to_owned()),
        }),
    );
    stack.script.push(
        LIST,
        Ok::<_, TreeReaderError>(ListingPage {
            items: Vec::new(),
            next: None,
        }),
    );
    let first = format!("/api/v1/nodes/{FOLDER}/children?after=a%20b%2Fc%C3%A9%2B%2B+&limit=500");
    let default_limit = format!("/api/v1/nodes/{FOLDER}/children");

    // Act
    let paged = stack.as_member("GET", &first, "").await;
    let last = stack.as_member("GET", &default_limit, "").await;

    // Assert
    assert_eq!(paged.status, 200);
    assert_eq!(paged.json()["items"].as_array().map(Vec::len), Some(2));
    assert_eq!(paged.json()["items"][1]["kind"], "folder");
    assert_eq!(paged.json()["next"], "next key/é");
    assert_eq!(last.json(), json!({"items": [], "next": null}));
    assert_eq!(
        stack.log(LIST),
        [
            format!("{LIST} ({MEMBER_ACCOUNT}, {FOLDER}, Some(\"a b/cé+++\"), 500)"),
            format!("{LIST} ({MEMBER_ACCOUNT}, {FOLDER}, None, 1000)"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn the_refusals_of_02_keep_their_codes() {
    // Arrange
    let stack = Stack::start().await;
    let script = &stack.script;
    script.push(GET_NODE, Err::<NodeView, _>(TreeReaderError::NotFound));
    script.push(LIST, Err::<ListingPage, _>(TreeReaderError::NotAFolder));
    script.push(LIST, Err::<ListingPage, _>(TreeReaderError::InvalidCursor));
    script.push(CREATE, Err::<NodeView, _>(TreeServiceError::NameConflict));
    script.push(
        CREATE,
        Err::<NodeView, _>(TreeServiceError::NameInvalid("slash".to_owned())),
    );
    script.push(CREATE, Err::<NodeView, _>(TreeServiceError::ParentDeleted));
    script.push(UPDATE, Err::<NodeView, _>(TreeServiceError::Cycle));
    script.push(UPDATE, Err::<NodeView, _>(TreeServiceError::IsRoot));
    script.push(
        DELETE,
        Err::<(), _>(TreeServiceError::EditedSince {
            current_revision: REVISION,
        }),
    );
    script.push(
        LOOKUP,
        Err::<Vec<RevisionMatch>, _>(TreeReaderError::BatchTooLarge),
    );
    script.push(
        FIND,
        Err::<Vec<RevisionRef>, _>(TreeServiceError::InvalidCursor),
    );
    let node = format!("/api/v1/nodes/{NODE}");
    let children = format!("/api/v1/nodes/{FOLDER}/children");
    let create = json!({"parent_id": FOLDER, "name": "x"}).to_string();
    let patch = json!({"name": "x"}).to_string();
    let lookup = json!({"content_hashes": [hex(1)]}).to_string();

    // Act
    let answers = [
        stack.as_member("GET", &node, "").await,
        stack.as_member("GET", &children, "").await,
        stack.as_member("GET", &children, "").await,
        stack.as_member("POST", "/api/v1/nodes", &create).await,
        stack.as_member("POST", "/api/v1/nodes", &create).await,
        stack.as_member("POST", "/api/v1/nodes", &create).await,
        stack.as_member("PATCH", &node, &patch).await,
        stack.as_member("PATCH", &node, &patch).await,
        stack.as_member("DELETE", &node, "").await,
        stack
            .as_member("POST", "/api/v1/revisions:lookup", &lookup)
            .await,
        stack.as_member("GET", "/api/v1/revisions", "").await,
    ];

    // Assert
    let seen: Vec<(u16, String)> = answers.iter().map(|a| (a.status, a.code())).collect();
    let expected = [
        (404, "not_found"),
        (422, "not_a_folder"),
        (422, "invalid_cursor"),
        (409, "name_conflict"),
        (422, "invalid_name"),
        (410, "parent_deleted"),
        (409, "cycle"),
        (400, "invalid_request"),
        (409, "edited_since"),
        (422, "batch_too_large"),
        (422, "invalid_cursor"),
    ];
    let expected: Vec<(u16, String)> = expected
        .iter()
        .map(|(s, c)| (*s, (*c).to_owned()))
        .collect();
    assert_eq!(seen, expected);
    stack.stop().await;
}

#[tokio::test]
async fn a_request_that_does_not_bind_never_reaches_a_port() {
    // Arrange
    let stack = Stack::start().await;
    let node = format!("/api/v1/nodes/{NODE}");
    let too_many = json!({"content_hashes": ["zz"]}).to_string();

    // Act
    let answers = [
        stack.as_member("GET", "/api/v1/nodes/not-a-uuid", "").await,
        stack
            .as_member("GET", "/api/v1/revisions/not-a-uuid", "")
            .await,
        stack
            .as_member(
                "POST",
                "/api/v1/nodes",
                &json!({"parent_id": FOLDER, "name": "x", "account_id": COPY}).to_string(),
            )
            .await,
        stack
            .as_member(
                "POST",
                "/api/v1/nodes",
                &json!({"parent_id": "7", "name": "x"}).to_string(),
            )
            .await,
        stack
            .as_member("PATCH", &node, &json!({"name": 5}).to_string())
            .await,
        stack.as_member("PATCH", &node, "not json").await,
        stack
            .as_member("DELETE", &format!("{node}?base_revision=7"), "")
            .await,
        stack
            .as_member(
                "GET",
                &format!("/api/v1/nodes/{FOLDER}/children?limit=many"),
                "",
            )
            .await,
        stack
            .as_member(
                "GET",
                &format!("/api/v1/nodes/{FOLDER}/children?after=%ZZ"),
                "",
            )
            .await,
        stack
            .as_member("POST", "/api/v1/revisions:lookup", &too_many)
            .await,
        stack
            .as_member("POST", "/api/v1/revisions:lookup", r#"{"hashes": []}"#)
            .await,
    ];

    // Assert
    for answer in &answers {
        assert_eq!(answer.status, 400);
        assert_eq!(answer.code(), "invalid_request");
    }
    assert_eq!(stack.script.log(), Vec::<String>::new());
    stack.stop().await;
}

#[tokio::test]
async fn an_update_passes_a_rename_and_a_move_alone_and_a_null_is_an_absent_member() {
    // Arrange
    let stack = Stack::start().await;
    for _ in 0..3 {
        stack
            .script
            .push(UPDATE, Ok::<_, TreeServiceError>(file_node(NODE)));
    }
    let node = format!("/api/v1/nodes/{NODE}");

    // Act
    stack
        .as_member("PATCH", &node, &json!({"name": "a"}).to_string())
        .await;
    stack
        .as_member("PATCH", &node, &json!({"parent_id": FOLDER}).to_string())
        .await;
    stack
        .as_member(
            "PATCH",
            &node,
            &json!({"name": null, "parent_id": null}).to_string(),
        )
        .await;

    // Assert
    let account = MEMBER_ACCOUNT;
    assert_eq!(
        stack.log(UPDATE),
        [
            format!("{UPDATE} ({account}, {NODE}, Some(\"a\"), None)"),
            format!("{UPDATE} ({account}, {NODE}, None, Some({FOLDER}))"),
            format!("{UPDATE} ({account}, {NODE}, None, None)"),
        ]
    );
    stack.stop().await;
}

#[tokio::test]
async fn a_lookup_of_a_thousand_hashes_is_one_port_call_and_client_meta_that_is_not_json_is_an_opaque_500()
 {
    // Arrange
    let stack = Stack::start().await;
    stack.script.push(
        LOOKUP,
        Ok::<_, TreeReaderError>(Vec::<RevisionMatch>::new()),
    );
    stack.script.push(
        GET_REVISION,
        Ok::<RevisionView, TreeReaderError>(revision_view(Some("{broken"))),
    );
    let hashes: Vec<String> = (0..1000).map(|n| format!("{n:064x}")).collect();

    // Act
    let lookup = stack
        .as_member(
            "POST",
            "/api/v1/revisions:lookup",
            &json!({"content_hashes": hashes}).to_string(),
        )
        .await;
    let corrupt = stack
        .as_member("GET", &format!("/api/v1/revisions/{REVISION}"), "")
        .await;

    // Assert
    assert_eq!((lookup.status, lookup.json()), (200, json!([])));
    assert_eq!(stack.script.calls(LOOKUP), 1);
    assert_eq!((corrupt.status, corrupt.code().as_str()), (500, "internal"));
    assert!(!String::from_utf8_lossy(&corrupt.body).contains("broken"));
    stack.stop().await;
}
