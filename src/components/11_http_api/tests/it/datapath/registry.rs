use serde_json::{Value, json};
use sunnycloud_http_api::registry::{
    AuthClass, BodyClass, HttpMethod, ListenerKind, RouteSpec, Timing, document, production_routes,
    validate,
};

use HttpMethod::{Delete, Get, Head, Patch, Post, Put};

fn spec(method: HttpMethod, path: &str) -> RouteSpec {
    production_routes()
        .into_iter()
        .map(|def| def.spec)
        .find(|spec| spec.method == method && spec.path == path)
        .unwrap_or_else(|| panic!("{} {path} is registered", method.key()))
}

/// (method, path, body class, state-changing, owner, success)
type Row = (HttpMethod, &'static str, BodyClass, bool, &'static str, u16);

const ROWS: [Row; 22] = [
    (
        Get,
        "/api/v1/nodes/root",
        BodyClass::None,
        false,
        "TreeReader::get_node",
        200,
    ),
    (
        Post,
        "/api/v1/nodes",
        BodyClass::Small,
        true,
        "TreeService::create_folder",
        201,
    ),
    (
        Get,
        "/api/v1/nodes/{id}",
        BodyClass::None,
        false,
        "TreeReader::get_node",
        200,
    ),
    (
        Patch,
        "/api/v1/nodes/{id}",
        BodyClass::Small,
        true,
        "TreeService::update_node",
        200,
    ),
    (
        Delete,
        "/api/v1/nodes/{id}",
        BodyClass::None,
        true,
        "TreeService::delete_node",
        204,
    ),
    (
        Get,
        "/api/v1/nodes/{id}/children",
        BodyClass::None,
        false,
        "TreeReader::list_children",
        200,
    ),
    (
        Get,
        "/api/v1/revisions/{id}",
        BodyClass::None,
        false,
        "TreeReader::get_revision",
        200,
    ),
    (
        Get,
        "/api/v1/revisions",
        BodyClass::None,
        false,
        "TreeService::find_revisions",
        200,
    ),
    (
        Post,
        "/api/v1/revisions:lookup",
        BodyClass::Query,
        false,
        "TreeReader::lookup_revisions",
        200,
    ),
    (
        Post,
        "/api/v1/uploads",
        BodyClass::Small,
        true,
        "UploadService::start_upload",
        200,
    ),
    (
        Get,
        "/api/v1/uploads/{upload_id}",
        BodyClass::None,
        false,
        "UploadService::get_upload",
        200,
    ),
    (
        Delete,
        "/api/v1/uploads/{upload_id}",
        BodyClass::None,
        true,
        "UploadService::abort_upload",
        204,
    ),
    (
        Put,
        "/api/v1/uploads/{upload_id}/blocks/{index}",
        BodyClass::Block,
        true,
        "UploadService::put_block",
        204,
    ),
    (
        Put,
        "/api/v1/staged-uploads/{staged_key}",
        BodyClass::Staged,
        true,
        "UploadService::stage_small",
        200,
    ),
    (
        Post,
        "/api/v1/commits",
        BodyClass::Commit,
        true,
        "CommitService::commit_upload",
        200,
    ),
    (
        Post,
        "/api/v1/commits:batch",
        BodyClass::Batch,
        true,
        "CommitService::commit_batch",
        200,
    ),
    (
        Get,
        "/api/v1/nodes/{node_id}/content",
        BodyClass::None,
        false,
        "DownloadService::open_revision_content",
        200,
    ),
    (
        Head,
        "/api/v1/nodes/{node_id}/content",
        BodyClass::None,
        false,
        "DownloadService::open_revision_content",
        200,
    ),
    (
        Get,
        "/api/v1/revisions/{revision_id}/content",
        BodyClass::None,
        false,
        "DownloadService::open_revision_content",
        200,
    ),
    (
        Head,
        "/api/v1/revisions/{revision_id}/content",
        BodyClass::None,
        false,
        "DownloadService::open_revision_content",
        200,
    ),
    (
        Get,
        "/api/v1/changes",
        BodyClass::None,
        false,
        "ChangeFeed::read",
        200,
    ),
    (
        Get,
        "/api/v1/changes/head",
        BodyClass::None,
        false,
        "ChangeFeed::head",
        200,
    ),
];

#[test]
fn every_tree_transfer_and_sync_operation_is_registered_with_the_declarations_of_the_surface_table()
{
    // Arrange
    let rows = ROWS;

    // Act
    let found: Vec<Row> = rows
        .iter()
        .map(|(method, path, ..)| {
            let spec = spec(*method, path);
            (
                spec.method,
                spec.path,
                spec.body,
                spec.state_changing,
                spec.owner,
                spec.success,
            )
        })
        .collect();

    // Assert
    assert_eq!(found, rows);
    for (method, path, ..) in rows {
        let spec = spec(method, path);
        assert_eq!(spec.auth, AuthClass::SessionOrDevice, "{path}");
        assert_eq!(spec.listener, ListenerKind::Public, "{path}");
        assert!(!spec.summary.is_empty(), "{path}");
    }
    assert_eq!(validate(&production_routes()), Ok(()));
}

#[test]
fn the_feed_is_a_long_poll_the_byte_routes_are_downloads_and_the_rest_have_the_request_clock() {
    // Arrange
    let rows = ROWS;

    // Act
    let timings: Vec<(&str, Timing)> = rows
        .iter()
        .map(|(method, path, ..)| (*path, spec(*method, path).timing))
        .collect();

    // Assert
    for (path, timing) in timings {
        let expected = if path == "/api/v1/changes" {
            Timing::LongPoll
        } else if path.ends_with("/content") {
            Timing::Download
        } else {
            Timing::Request
        };
        assert_eq!(timing, expected, "{path}");
    }
}

#[test]
fn the_lookup_of_1000_hashes_fits_its_body_class_and_the_commit_classes_are_the_documented_ones() {
    // Arrange
    let body = json!({"content_hashes": vec!["0".repeat(64); 1000]}).to_string();

    // Act
    let lookup = spec(Post, "/api/v1/revisions:lookup");

    // Assert
    assert!(body.len() < 80_000, "{}", body.len());
    assert!(body.len() as u64 <= lookup.body.limit_bytes());
    assert!(!lookup.state_changing);
    assert_eq!(spec(Post, "/api/v1/commits").body.limit_bytes(), 4 << 20);
    assert_eq!(
        spec(Post, "/api/v1/commits:batch").body.limit_bytes(),
        4 << 20
    );
    assert_eq!(
        spec(Put, "/api/v1/uploads/{upload_id}/blocks/{index}")
            .body
            .limit_bytes(),
        2 << 20
    );
}

#[test]
fn the_document_lists_every_identifier_of_the_data_path() {
    // Arrange
    let routes = production_routes();
    let parsed: Value = serde_json::from_slice(document(&routes).as_bytes()).expect("document");
    let ids = |path: &str, method: &str| parsed["paths"][path][method]["x-id-params"].clone();

    // Act
    let found = [
        ids("/api/v1/nodes", "post"),
        ids("/api/v1/nodes/{id}", "patch"),
        ids("/api/v1/nodes/{id}", "delete"),
        ids("/api/v1/nodes/{id}/children", "get"),
        ids("/api/v1/uploads/{upload_id}/blocks/{index}", "put"),
        ids("/api/v1/staged-uploads/{staged_key}", "put"),
        ids("/api/v1/commits", "post"),
        ids("/api/v1/commits:batch", "post"),
        ids("/api/v1/revisions/{revision_id}/content", "head"),
        ids("/api/v1/changes", "get"),
    ];

    // Assert
    assert_eq!(
        found,
        [
            json!([{"name": "parent_id", "in": "body"}]),
            json!([{"name": "id", "in": "path"}, {"name": "parent_id", "in": "body"}]),
            json!([{"name": "id", "in": "path"}, {"name": "base_revision", "in": "query"}]),
            json!([{"name": "id", "in": "path"}]),
            json!([{"name": "upload_id", "in": "path"}]),
            json!([{"name": "staged_key", "in": "path"}]),
            json!([
                {"name": "upload_id", "in": "body"},
                {"name": "parent_id", "in": "body"},
                {"name": "base_revision", "in": "body"}
            ]),
            json!([
                {"name": "staged_key", "in": "body"},
                {"name": "parent_id", "in": "body"},
                {"name": "base_revision", "in": "body"}
            ]),
            json!([{"name": "revision_id", "in": "path"}]),
            json!([]),
        ]
    );
}
