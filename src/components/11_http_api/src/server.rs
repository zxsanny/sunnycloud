use std::future::Future;
use std::net::SocketAddr;

use axum::Router;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::accept::AcceptLoop;
use crate::config::HttpConfig;
use crate::error::{HttpServerError, Listener};
use crate::services::ApiServices;

/// The OpenAPI document as served by `GET /api/v1/openapi.json`. The route registry that fills it is
/// E-21/c10; until then the document describes no operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenApiDocument(Vec<u8>);

impl OpenApiDocument {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// The API server: four listeners over one accept loop (public, internal, and later ops and test-clock).
pub struct HttpServer {
    services: ApiServices,
    config: HttpConfig,
    accept: AcceptLoop,
}

/// A server whose listeners are bound, with their local addresses known.
pub struct BoundServer {
    accept: AcceptLoop,
    public: TcpListener,
    public_addr: SocketAddr,
    public_app: Router,
    internal: TcpListener,
    internal_addr: SocketAddr,
    internal_app: Router,
}

impl HttpServer {
    pub fn new(services: ApiServices, config: HttpConfig) -> Result<Self, HttpServerError> {
        let accept = AcceptLoop::new(&config, &services.metrics)?;
        Ok(HttpServer {
            services,
            config,
            accept,
        })
    }

    /// Binds the public and internal listeners. Split from [`HttpServer::run`] so a caller that bound
    /// port 0 can read the addresses back.
    pub async fn bind(self) -> Result<BoundServer, HttpServerError> {
        let (public, public_addr) = bind(Listener::Public, self.config.public_addr).await?;
        let (internal, internal_addr) = bind(Listener::Internal, self.config.internal_addr).await?;
        Ok(BoundServer {
            accept: self.accept,
            public,
            public_addr,
            public_app: public_router(&self.services),
            internal,
            internal_addr,
            internal_app: internal_router(&self.services),
        })
    }

    /// Binds every listener, serves until `shutdown_signal` completes, then stops accepting and returns.
    pub async fn run(
        self,
        shutdown_signal: impl Future<Output = ()>,
    ) -> Result<(), HttpServerError> {
        self.bind().await?.serve(shutdown_signal).await
    }

    pub fn openapi(&self) -> OpenApiDocument {
        let document = concat!(
            r#"{"openapi":"3.1.0","info":{"title":"sunnycloud","version":""#,
            env!("CARGO_PKG_VERSION"),
            r#""},"paths":{}}"#
        );
        OpenApiDocument(document.as_bytes().to_vec())
    }
}

impl BoundServer {
    pub fn public_addr(&self) -> SocketAddr {
        self.public_addr
    }

    pub fn internal_addr(&self) -> SocketAddr {
        self.internal_addr
    }

    /// Serves until `shutdown_signal` completes, then stops accepting. Connections already open are left to
    /// finish; closing them within the grace is E-21/c17.
    pub async fn serve(
        self,
        shutdown_signal: impl Future<Output = ()>,
    ) -> Result<(), HttpServerError> {
        let stop = CancellationToken::new();
        let public = self
            .accept
            .serve(self.public, self.public_app, stop.clone());
        let internal = self
            .accept
            .serve(self.internal, self.internal_app, stop.clone());
        let signal = async {
            shutdown_signal.await;
            stop.cancel();
        };
        tokio::join!(public, internal, signal);
        Ok(())
    }
}

async fn bind(
    listener: Listener,
    addr: SocketAddr,
) -> Result<(TcpListener, SocketAddr), HttpServerError> {
    let bind_error = |source| HttpServerError::Bind {
        listener,
        addr,
        source,
    };
    let bound = TcpListener::bind(addr).await.map_err(bind_error)?;
    let local = bound.local_addr().map_err(bind_error)?;
    Ok((bound, local))
}

fn public_router(_services: &ApiServices) -> Router {
    Router::new()
}

fn internal_router(_services: &ApiServices) -> Router {
    Router::new()
}
