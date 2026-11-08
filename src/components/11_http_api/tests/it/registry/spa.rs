use sunnycloud_http_api::{HttpServer, SpaBundle};

use crate::harness::router_server::request;
use crate::harness::{Script, default_config, new_metrics, stub_services};

const INDEX: &str = "<!doctype html><title>sunnycloud</title>";
const SCRIPT: &str = "console.log('app');";

struct Running {
    public: std::net::SocketAddr,
    internal: std::net::SocketAddr,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), sunnycloud_http_api::HttpServerError>>,
}

async fn start(bundle: Option<SpaBundle>) -> Running {
    let mut server = HttpServer::new(
        stub_services(&Script::default(), new_metrics()),
        default_config(),
    )
    .expect("server builds");
    if let Some(bundle) = bundle {
        server = server.with_spa(bundle);
    }
    let bound = server.bind().await.expect("listeners bind");
    let (public, internal) = (bound.public_addr(), bound.internal_addr());
    let (stop, signal) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(bound.serve(async move {
        signal.await.ok();
    }));
    Running {
        public,
        internal,
        stop,
        task,
    }
}

impl Running {
    async fn finish(self) {
        self.stop.send(()).expect("server is running");
        self.task
            .await
            .expect("serve joins")
            .expect("serve returns");
    }
}

fn bundle() -> SpaBundle {
    SpaBundle::new(INDEX).with_asset("/assets/app.js", "text/javascript", SCRIPT)
}

#[tokio::test]
async fn a_non_api_path_gets_the_bundle_index() {
    // Arrange
    let server = start(Some(bundle())).await;

    // Act
    let deep = request(server.public, "GET", "/files/a/b").await;
    let root = request(server.public, "GET", "/").await;

    // Assert
    for answer in [&deep, &root] {
        assert_eq!(answer.status, 200);
        assert_eq!(
            answer.content_type.as_deref(),
            Some("text/html; charset=utf-8")
        );
        assert_eq!(answer.body, INDEX.as_bytes());
    }
    server.finish().await;
}

#[tokio::test]
async fn a_bundled_file_is_served_as_itself_and_head_has_no_body() {
    // Arrange
    let server = start(Some(bundle())).await;

    // Act
    let script = request(server.public, "GET", "/assets/app.js").await;
    let head = request(server.public, "HEAD", "/files/a/b").await;

    // Assert
    assert_eq!(script.status, 200);
    assert_eq!(script.content_type.as_deref(), Some("text/javascript"));
    assert_eq!(script.body, SCRIPT.as_bytes());
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    assert_eq!(head.content_length, Some(INDEX.len().to_string()));
    server.finish().await;
}

#[tokio::test]
async fn an_unknown_api_path_is_a_404_problem_never_the_index() {
    // Arrange
    let server = start(Some(bundle())).await;

    // Act
    let answers = [
        request(server.public, "GET", "/api/v1/nope").await,
        request(server.public, "GET", "/api/v1/vaults/1").await,
        request(server.public, "GET", "/api/v2/server-info").await,
        request(server.public, "GET", "/api").await,
        request(server.public, "POST", "/files/a/b").await,
    ];

    // Assert
    for answer in &answers {
        assert_eq!(answer.status, 404);
        assert_eq!(
            answer.content_type.as_deref(),
            Some("application/problem+json")
        );
        let body = answer.json();
        assert_eq!(body["code"], "not_found");
        assert_eq!(body["status"], 404);
        assert_ne!(answer.body, INDEX.as_bytes());
    }
    server.finish().await;
}

#[tokio::test]
async fn without_a_bundle_every_unknown_path_is_a_404_problem() {
    // Arrange
    let server = start(None).await;

    // Act
    let answer = request(server.public, "GET", "/files/a/b").await;

    // Assert
    assert_eq!(answer.status, 404);
    assert_eq!(answer.json()["code"], "not_found");
    server.finish().await;
}

#[tokio::test]
async fn the_bundle_is_not_served_on_the_internal_listener() {
    // Arrange
    let server = start(Some(bundle())).await;

    // Act
    let answer = request(server.internal, "GET", "/files/a/b").await;

    // Assert
    assert_eq!(answer.status, 404);
    assert_eq!(answer.json()["code"], "not_found");
    server.finish().await;
}

#[tokio::test]
async fn the_known_api_routes_still_win_over_the_fallback() {
    // Arrange
    let server = start(Some(bundle())).await;

    // Act
    let info = request(server.public, "GET", "/api/v1/server-info").await;

    // Assert
    assert_eq!(info.status, 200);
    assert_eq!(info.json()["block_size"], 2_097_152);
    server.finish().await;
}
