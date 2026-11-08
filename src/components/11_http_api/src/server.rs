use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use sc_observability::{Liveness, ReadinessRegistry, ops_router};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::accept::AcceptLoop;
use crate::config::HttpConfig;
use crate::csrf::CsrfGuard;
use crate::error::{HttpServerError, Listener};
use crate::limits::BodyLimits;
use crate::metrics::HttpMetrics;
use crate::middleware::Middleware;
use crate::registry::{
    OpenApiDocument, RouteContext, RouteDef, document, mount, production_routes, validate,
};
use crate::services::ApiServices;
use crate::spa::SpaBundle;
use crate::version::VersionGate;

/// The document of this build, as `HttpServer::openapi` returns and `GET /api/v1/openapi.json` serves it.
pub fn current_document() -> OpenApiDocument {
    document(&production_routes())
}

/// The API server: its listeners (public, internal, ops, and later test-clock) over one accept loop.
pub struct HttpServer {
    services: Arc<ApiServices>,
    config: HttpConfig,
    accept: AcceptLoop,
    http_metrics: Arc<HttpMetrics>,
    middleware: Middleware,
    version_gate: Arc<VersionGate>,
    csrf: Arc<CsrfGuard>,
    body_limits: Arc<BodyLimits>,
    routes: Vec<RouteDef>,
    openapi: OpenApiDocument,
    spa: Option<Arc<SpaBundle>>,
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
    ops: TcpListener,
    ops_addr: SocketAddr,
    ops_app: Router,
    liveness: Liveness,
}

impl HttpServer {
    pub fn new(services: ApiServices, config: HttpConfig) -> Result<Self, HttpServerError> {
        let accept = AcceptLoop::new(&config, &services.metrics)?;
        let http_metrics = Arc::new(HttpMetrics::register(&services.metrics)?);
        let routes = production_routes();
        validate(&routes)?;
        let middleware = Middleware::new(
            &config,
            Arc::clone(&services.clock),
            Arc::clone(&http_metrics),
            routes.iter().map(|route| route.spec.path),
        )?;
        let version_gate = VersionGate::new(
            &config,
            Arc::clone(&http_metrics),
            Arc::clone(&services.clock),
        )?;
        let csrf = CsrfGuard::new(&config.public_url, Arc::clone(&services.clock))?;
        let body_limits = BodyLimits::new(
            &config,
            Arc::clone(&http_metrics),
            Arc::clone(&services.clock),
        );
        let openapi = document(&routes);
        Ok(HttpServer {
            services: Arc::new(services),
            config,
            accept,
            http_metrics,
            middleware,
            version_gate,
            csrf,
            body_limits,
            routes,
            openapi,
            spa: None,
        })
    }

    /// Serves the bundled web app: its static files, and its index for every other non-`/api` path.
    pub fn with_spa(mut self, bundle: SpaBundle) -> Self {
        self.spa = Some(Arc::new(bundle));
        self
    }

    /// The HTTP metric families registered by [`HttpServer::new`], for the layers that increment them.
    pub fn http_metrics(&self) -> Arc<HttpMetrics> {
        Arc::clone(&self.http_metrics)
    }

    /// Binds the public, internal and ops listeners. Split from [`HttpServer::run`] so a caller that bound
    /// port 0 can read the addresses back.
    pub async fn bind(self) -> Result<BoundServer, HttpServerError> {
        let (public, public_addr) = bind(Listener::Public, self.config.public_addr).await?;
        let (internal, internal_addr) = bind(Listener::Internal, self.config.internal_addr).await?;
        let (ops, ops_addr) = bind(Listener::Ops, self.config.ops_addr).await?;
        let liveness = Liveness::new();
        let context = RouteContext {
            services: Arc::clone(&self.services),
            openapi: self.openapi.clone(),
            server_info: self.config.server_info.clone(),
            credentials: self.config.credentials.clone(),
            version_gate: Arc::clone(&self.version_gate),
            csrf: Arc::clone(&self.csrf),
            body_limits: Arc::clone(&self.body_limits),
        };
        let routers = mount(&self.routes, &context, self.spa.clone());
        Ok(BoundServer {
            accept: self.accept,
            public,
            public_addr,
            public_app: self.middleware.wrap(routers.public),
            internal,
            internal_addr,
            internal_app: self.middleware.wrap(routers.internal),
            ops,
            ops_addr,
            ops_app: ops_app(&self.services, liveness.clone()),
            liveness,
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
        self.openapi.clone()
    }
}

impl BoundServer {
    pub fn public_addr(&self) -> SocketAddr {
        self.public_addr
    }

    pub fn internal_addr(&self) -> SocketAddr {
        self.internal_addr
    }

    pub fn ops_addr(&self) -> SocketAddr {
        self.ops_addr
    }

    /// Serves until `shutdown_signal` completes, turns `/readyz` to 503, then stops accepting. Connections
    /// already open are left to finish; closing them within the grace is E-21/c17.
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
        let ops = self.accept.serve(self.ops, self.ops_app, stop.clone());
        let liveness = self.liveness;
        let signal = async {
            shutdown_signal.await;
            liveness.shut_down();
            stop.cancel();
        };
        tokio::join!(public, internal, ops, signal);
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

fn ops_app(services: &ApiServices, liveness: Liveness) -> Router {
    let mut readiness = ReadinessRegistry::new();
    for check in &services.readiness {
        readiness.add(Arc::clone(check));
    }
    ops_router(Arc::clone(&services.metrics), readiness, liveness)
}
