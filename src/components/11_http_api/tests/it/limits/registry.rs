use sunnycloud_http_api::registry::{
    AuthClass, BodyClass, HttpMethod, Param, ParamType, RouteDef, RouteSpec, validate,
};

const WAIT: &[Param] = &[Param::query("wait", ParamType::Integer)];

fn spec(method: HttpMethod, path: &'static str) -> RouteSpec {
    RouteSpec::new(
        method,
        path,
        "ChangeFeed::read",
        AuthClass::SessionOrDevice,
        BodyClass::None,
    )
    .summary("A test operation")
}

fn no_content(
    _context: &sunnycloud_http_api::registry::RouteContext,
    filter: axum::routing::MethodFilter,
) -> axum::routing::MethodRouter {
    axum::routing::on(filter, || async { axum::http::StatusCode::NO_CONTENT })
}

fn problems(spec: RouteSpec) -> Vec<String> {
    validate(&[RouteDef::new(spec, no_content)])
        .err()
        .map(|error| error.0)
        .unwrap_or_default()
}

#[test]
fn a_long_poll_needs_a_wait_parameter_and_a_download_needs_a_read_method() {
    // Arrange
    let bare = spec(HttpMethod::Get, "/api/v1/changes").long_poll();
    let waiting = spec(HttpMethod::Get, "/api/v1/changes")
        .long_poll()
        .params(WAIT);
    let posted = RouteSpec::new(
        HttpMethod::Post,
        "/api/v1/commits",
        "CommitService::commit_upload",
        AuthClass::SessionOrDevice,
        BodyClass::Commit,
    )
    .summary("A test operation")
    .download();

    // Act
    let (bare, waiting, posted) = (problems(bare), problems(waiting), problems(posted));

    // Assert
    assert!(bare.iter().any(|line| line.contains("wait")), "{bare:?}");
    assert!(waiting.is_empty(), "{waiting:?}");
    assert!(
        posted.iter().any(|line| line.contains("download")),
        "{posted:?}"
    );
}
