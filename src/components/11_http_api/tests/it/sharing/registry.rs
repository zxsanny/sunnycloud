use serde_json::Value;
use sunnycloud_http_api::registry::{
    AuthClass, BodyClass, HttpMethod, ListenerKind, RouteSpec, Timing, document, production_routes,
    validate,
};

fn spec(method: HttpMethod, path: &str) -> RouteSpec {
    production_routes()
        .into_iter()
        .map(|def| def.spec)
        .find(|spec| spec.method == method && spec.path == path)
        .unwrap_or_else(|| panic!("{} {path} is registered", method.key()))
}

/// (method, path, auth, body, state-changing, owner, success)
type Row = (
    HttpMethod,
    &'static str,
    AuthClass,
    BodyClass,
    bool,
    &'static str,
    u16,
);

const ROWS: [Row; 12] = [
    (
        HttpMethod::Post,
        "/api/v1/shares",
        AuthClass::SessionOnly,
        BodyClass::Small,
        true,
        "ShareService::create",
        201,
    ),
    (
        HttpMethod::Get,
        "/api/v1/shares",
        AuthClass::SessionOnly,
        BodyClass::None,
        false,
        "ShareService::list",
        200,
    ),
    (
        HttpMethod::Delete,
        "/api/v1/shares/{id}",
        AuthClass::SessionOnly,
        BodyClass::None,
        true,
        "ShareService::revoke",
        204,
    ),
    (
        HttpMethod::Get,
        "/api/v1/public-shares/{token}",
        AuthClass::Public,
        BodyClass::None,
        false,
        "PublicShareService::open",
        200,
    ),
    (
        HttpMethod::Post,
        "/api/v1/public-shares/{token}/password-verifications",
        AuthClass::Public,
        BodyClass::Auth,
        true,
        "PublicShareService::verify_password",
        204,
    ),
    (
        HttpMethod::Get,
        "/api/v1/public-shares/{token}/content",
        AuthClass::ShareGrant,
        BodyClass::None,
        false,
        "PublicShareService::authorize_content",
        200,
    ),
    (
        HttpMethod::Head,
        "/api/v1/public-shares/{token}/content",
        AuthClass::ShareGrant,
        BodyClass::None,
        false,
        "PublicShareService::authorize_content",
        200,
    ),
    (
        HttpMethod::Get,
        "/api/v1/nodes/{node_id}/thumbnail",
        AuthClass::SessionOrDevice,
        BodyClass::None,
        false,
        "DerivedOpener::open_derived",
        200,
    ),
    (
        HttpMethod::Head,
        "/api/v1/nodes/{node_id}/thumbnail",
        AuthClass::SessionOrDevice,
        BodyClass::None,
        false,
        "DerivedOpener::open_derived",
        200,
    ),
    (
        HttpMethod::Get,
        "/api/v1/nodes/{node_id}/transcode",
        AuthClass::SessionOrDevice,
        BodyClass::None,
        false,
        "DerivedOpener::open_derived",
        200,
    ),
    (
        HttpMethod::Head,
        "/api/v1/nodes/{node_id}/transcode",
        AuthClass::SessionOrDevice,
        BodyClass::None,
        false,
        "DerivedOpener::open_derived",
        200,
    ),
    (
        HttpMethod::Post,
        "/api/v1/thumbnail-states",
        AuthClass::SessionOrDevice,
        BodyClass::Query,
        false,
        "ThumbnailStateSource::states",
        200,
    ),
];

#[test]
fn every_sharing_and_media_operation_is_registered_with_the_declarations_of_the_surface_table() {
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
                spec.auth,
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
        assert_eq!(spec.listener, ListenerKind::Public, "{path}");
        assert!(!spec.summary.is_empty(), "{path}");
    }
}

#[test]
fn the_byte_routes_are_downloads_and_the_json_routes_are_not() {
    // Arrange
    let byte_routes = ["/content", "/thumbnail", "/transcode"];

    // Act
    let timings: Vec<(&str, Timing)> = ROWS
        .iter()
        .map(|(_, path, ..)| (*path, spec_timing(path)))
        .collect();

    // Assert
    for (path, timing) in timings {
        let download = byte_routes.iter().any(|tail| path.ends_with(tail));
        assert_eq!(timing == Timing::Download, download, "{path}");
    }
}

fn spec_timing(path: &str) -> Timing {
    production_routes()
        .into_iter()
        .map(|def| def.spec)
        .find(|spec| spec.path == path)
        .map(|spec| spec.timing)
        .expect("registered")
}

#[test]
fn the_document_lists_every_identifier_and_the_registry_validates() {
    // Arrange
    let routes = production_routes();
    let parsed: Value = serde_json::from_slice(document(&routes).as_bytes()).expect("document");
    let ids = |path: &str, method: &str| parsed["paths"][path][method]["x-id-params"].clone();

    // Act
    let (create, list, revoke, visitor, thumbnail) = (
        ids("/api/v1/shares", "post"),
        ids("/api/v1/shares", "get"),
        ids("/api/v1/shares/{id}", "delete"),
        ids("/api/v1/public-shares/{token}/content", "get"),
        ids("/api/v1/nodes/{node_id}/thumbnail", "head"),
    );

    // Assert
    assert_eq!(validate(&routes), Ok(()));
    assert_eq!(
        create,
        serde_json::json!([{"name": "node_id", "in": "body"}])
    );
    assert_eq!(
        list,
        serde_json::json!([{"name": "node_id", "in": "query"}])
    );
    assert_eq!(revoke, serde_json::json!([{"name": "id", "in": "path"}]));
    assert_eq!(
        visitor,
        serde_json::json!([{"name": "token", "in": "path"}])
    );
    assert_eq!(
        thumbnail,
        serde_json::json!([{"name": "node_id", "in": "path"}])
    );
}
