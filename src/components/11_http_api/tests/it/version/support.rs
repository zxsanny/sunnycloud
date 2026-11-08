use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::Request;
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper::header::HeaderMap;
use hyper_util::rt::TokioIo;
use sc_observability::Metrics;
use serde_json::Value;
use sunnycloud_http_api::registry::{
    RouteContext, RouteDef, Routers, document, mount, production_routes,
};
use sunnycloud_http_api::{
    ApiServices, BodyLimits, CsrfGuard, HttpConfig, HttpMetrics, ServerInfo, VersionGate,
};
use tokio::net::TcpStream;

use crate::auth::support::{ScriptedAuthenticator, ScriptedWorkerAuth, class_routes};
use crate::harness::router_server::RouterServer;
use crate::harness::{Script, default_config, new_metrics, sample_server_info, stub_services};

pub const UPDATE_URL: &str = "https://sunnycloud.test/download";
pub const PROTECTED: &str = "/api/v1/probe/session-or-device";
pub const PUBLIC: &str = "/api/v1/probe/public";
pub const SERVER_INFO: &str = "/api/v1/server-info";

/// A config whose minimum is `minimum` and whose `ServerInfo` serves the same text.
pub fn gated_config(minimum: &str) -> HttpConfig {
    HttpConfig {
        min_client_version: Some(minimum.to_owned()),
        update_url: Some(UPDATE_URL.to_owned()),
        server_info: ServerInfo {
            max_lanes: 3,
            batch_max_entries: 250,
            max_staged_per_account: 7,
            longpoll_max_wait: Duration::from_secs(20),
            ..sample_server_info(minimum)
        },
        ..default_config()
    }
}

/// One answer with its headers.
pub struct Reply {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("the body is JSON")
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

pub async fn get(addr: SocketAddr, path: &str, headers: &[(&str, &str)]) -> Reply {
    let stream = TcpStream::connect(addr).await.expect("tcp connect");
    let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
        .await
        .expect("h1 handshake");
    tokio::spawn(connection);
    let mut builder = Request::builder()
        .uri(path)
        .header("host", "sunnycloud.test");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = builder.body(Empty::<Bytes>::new()).expect("request");
    let response = sender.send_request(request).await.expect("h1 response");
    let (parts, body) = response.into_parts();
    let body = body.collect().await.expect("body").to_bytes().to_vec();
    Reply {
        status: parts.status.as_u16(),
        headers: parts.headers,
        body,
    }
}

/// `GET path` with a bearer token and, unless `client` is `None`, `X-Sunnycloud-Client: client`.
pub async fn get_as(addr: SocketAddr, path: &str, client: Option<&str>, token: &str) -> Reply {
    let bearer = format!("Bearer {token}");
    let mut headers = vec![("authorization", bearer.as_str())];
    if let Some(client) = client {
        headers.push(("x-sunnycloud-client", client));
    }
    get(addr, path, &headers).await
}

/// One probe route per auth class plus the production system routes, mounted behind the gate of `config`.
pub struct Gated {
    pub public: RouterServer,
    pub internal: RouterServer,
    pub authenticator: Arc<ScriptedAuthenticator>,
    pub worker_auth: Arc<ScriptedWorkerAuth>,
    pub metrics: Arc<Metrics>,
}

impl Gated {
    pub async fn start(config: HttpConfig) -> Gated {
        let authenticator = Arc::new(ScriptedAuthenticator::default());
        let worker_auth = Arc::new(ScriptedWorkerAuth::default());
        let metrics = new_metrics();
        let http_metrics = Arc::new(HttpMetrics::register(&metrics).expect("http families"));
        let services = ApiServices {
            authenticator: authenticator.clone(),
            worker_auth: worker_auth.clone(),
            ..stub_services(&Script::default(), metrics.clone())
        };
        let version_gate = VersionGate::new(&config, http_metrics.clone(), services.clock.clone())
            .expect("gate builds");
        let csrf = CsrfGuard::new(&config.public_url, services.clock.clone()).expect("origin");
        let body_limits = BodyLimits::new(&config, http_metrics, services.clock.clone());
        let defs: Vec<RouteDef> = class_routes()
            .into_iter()
            .chain(production_routes())
            .collect();
        let context = RouteContext {
            services: Arc::new(services),
            openapi: document(&defs),
            server_info: config.server_info,
            credentials: config.credentials,
            version_gate,
            csrf,
            body_limits,
        };
        let Routers { public, internal } = mount(&defs, &context, None);
        Gated {
            public: RouterServer::start(public).await,
            internal: RouterServer::start(internal).await,
            authenticator,
            worker_auth,
            metrics,
        }
    }

    pub async fn stop(self) {
        self.public.stop().await;
        self.internal.stop().await;
    }
}
