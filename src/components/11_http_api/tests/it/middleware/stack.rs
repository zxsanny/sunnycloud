use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Extension, Path};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use sc_error::{BusinessError, BusinessParams, CorrelationId, DomainError, ErrorCode};
use sc_observability::Metrics;
use sunnycloud_http_api::{AcceptLoop, ApiProblem, ClientIp, HttpConfig, HttpMetrics, Middleware};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::harness::{EVENT_BOUND, Script, default_config, new_metrics, stub_services};

pub const NODE_ROUTE: &str = "/api/v1/nodes/{id}";
pub const SHARE_ROUTE: &str = "/api/v1/public-shares/{token}";
pub const SERVER_INFO: &str = "/api/v1/server-info";

const TEMPLATES: [&str; 3] = [NODE_ROUTE, SHARE_ROUTE, SERVER_INFO];

/// The real `Middleware` around probe routes, served by the shared accept loop.
pub struct Stack {
    pub addr: SocketAddr,
    pub metrics: Arc<Metrics>,
    pub http_metrics: Arc<HttpMetrics>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl Stack {
    pub async fn start(trusted_proxies: &[&str]) -> Stack {
        let config = HttpConfig {
            trusted_proxies: trusted_proxies
                .iter()
                .map(|range| (*range).to_owned())
                .collect(),
            ..default_config()
        };
        let metrics = new_metrics();
        let http_metrics = Arc::new(HttpMetrics::register(&metrics).expect("http families"));
        let services = stub_services(&Script::default(), metrics.clone());
        let middleware = Middleware::new(
            &config,
            services.clock.clone(),
            http_metrics.clone(),
            TEMPLATES,
        )
        .expect("middleware builds");
        let accept = AcceptLoop::new(&config, &metrics).expect("accept loop builds");
        let listener = TcpListener::bind(config.public_addr).await.expect("binds");
        let addr = listener.local_addr().expect("address");
        let stop = CancellationToken::new();
        let app = middleware.wrap(probe_routes());
        let task = tokio::spawn({
            let stop = stop.clone();
            async move { accept.serve(listener, app, stop).await }
        });
        Stack {
            addr,
            metrics,
            http_metrics,
            stop,
            task,
        }
    }

    pub async fn stop(self) {
        self.stop.cancel();
        tokio::time::timeout(EVENT_BOUND, self.task)
            .await
            .expect("the accept loop stops")
            .expect("the accept loop does not panic");
    }

    pub async fn series(&self, family: &str) -> Vec<String> {
        let text = self.metrics.render().await.expect("render metrics");
        text.lines()
            .filter(|line| line.starts_with(family))
            .map(str::to_owned)
            .collect()
    }
}

fn probe_routes() -> Router {
    Router::new()
        .route(SERVER_INFO, get(|| async { "{}" }))
        .route(NODE_ROUTE, get(node))
        .route(SHARE_ROUTE, get(|| async { StatusCode::OK }))
        .route("/probe/panic", get(|| async { panic_now() }))
        .route("/probe/slow", get(slow))
        .route("/probe/client-ip", get(client_ip))
        .route("/probe/status/{code}", get(status))
        .route("/probe/problem/{code}", get(problem))
        .route("/probe/private", get(private))
}

fn panic_now() -> StatusCode {
    panic!("probe handler panic with secret-payload");
}

async fn node(Path(id): Path<String>, Extension(request_id): Extension<CorrelationId>) -> Response {
    if id == "missing" {
        return ApiProblem::new(
            DomainError::Business(BusinessError::not_found()),
            request_id,
        )
        .into_response();
    }
    StatusCode::OK.into_response()
}

async fn slow() -> StatusCode {
    tokio::time::sleep(Duration::from_millis(300)).await;
    StatusCode::OK
}

async fn client_ip(Extension(ClientIp(address)): Extension<ClientIp>) -> String {
    address.to_string()
}

async fn status(Path(code): Path<u16>) -> StatusCode {
    StatusCode::from_u16(code).expect("a status code")
}

async fn problem(
    Path(code): Path<u16>,
    Extension(request_id): Extension<CorrelationId>,
) -> Response {
    let error_code = match code {
        401 => ErrorCode::Unauthenticated,
        413 => ErrorCode::SizeTooLarge,
        426 => ErrorCode::UpgradeRequired,
        500 => ErrorCode::Internal,
        _ => ErrorCode::NotFound,
    };
    let error = BusinessError::new(error_code, BusinessParams::default());
    ApiProblem::new(DomainError::Business(error), request_id).into_response()
}

async fn private() -> Response {
    let mut response = StatusCode::OK.into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("private"));
    response
}
