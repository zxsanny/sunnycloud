use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use hyper::Request;
use hyper::body::Incoming;
use hyper::service::Service;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::{TowerToHyperService, TowerToHyperServiceFuture};
use sc_observability::{Metrics, log_debug, log_warn};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use crate::config::HttpConfig;
use crate::error::HttpServerError;
use crate::gate::{ConnectionGate, ConnectionGuard};
use crate::metrics::ConnectionMetrics;
use crate::middleware::PeerAddr;
use crate::write_idle::WriteIdle;

/// Pause after an accept failure that is not about one peer (descriptor or memory exhaustion), so the loop
/// does not spin on it.
const ACCEPT_FAULT_BACKOFF: Duration = Duration::from_secs(1);

/// The values `HttpConfig` hands to the connection builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConnectionSettings {
    pub(crate) h1_header_timeout: Duration,
    pub(crate) h2_stream_window: u32,
    pub(crate) h2_conn_window: u32,
    pub(crate) h2_max_streams: u32,
    pub(crate) h2_max_pending_reset: usize,
    pub(crate) h2_max_header_list: u32,
    pub(crate) h2_keepalive_interval: Duration,
    pub(crate) h2_keepalive_timeout: Duration,
    pub(crate) download_write_idle: Duration,
}

impl ConnectionSettings {
    pub(crate) fn from_config(config: &HttpConfig) -> Self {
        ConnectionSettings {
            h1_header_timeout: config.h1_header_timeout,
            h2_stream_window: config.h2_stream_window,
            h2_conn_window: config.h2_conn_window,
            h2_max_streams: config.h2_max_streams,
            h2_max_pending_reset: config.h2_max_pending_reset,
            h2_max_header_list: config.h2_max_header_list,
            h2_keepalive_interval: config.h2_keepalive_interval,
            h2_keepalive_timeout: config.h2_keepalive_timeout,
            download_write_idle: config.download_write_idle,
        }
    }

    fn builder(&self) -> Builder<TokioExecutor> {
        let mut builder = Builder::new(TokioExecutor::new());
        builder
            .http1()
            .timer(TokioTimer::new())
            .header_read_timeout(self.h1_header_timeout);
        builder
            .http2()
            .timer(TokioTimer::new())
            .initial_stream_window_size(self.h2_stream_window)
            .initial_connection_window_size(self.h2_conn_window)
            .max_concurrent_streams(self.h2_max_streams)
            .max_pending_accept_reset_streams(self.h2_max_pending_reset)
            .max_header_list_size(self.h2_max_header_list)
            .keep_alive_interval(self.h2_keepalive_interval)
            .keep_alive_timeout(self.h2_keepalive_timeout);
        builder
    }
}

/// The loop every listener runs. Clones share the connection cap, so one `AcceptLoop` serves all the
/// listeners of a server under one `max_connections`.
#[derive(Clone)]
pub struct AcceptLoop {
    gate: Arc<ConnectionGate>,
    builder: Builder<TokioExecutor>,
    write_idle: Duration,
}

impl AcceptLoop {
    /// Registers `connections_open` and `connections_rejected_total` in `metrics`, so one registry serves
    /// one `AcceptLoop`.
    pub fn new(config: &HttpConfig, metrics: &Metrics) -> Result<Self, HttpServerError> {
        config.check()?;
        let gate = ConnectionGate::new(
            config.max_connections,
            ConnectionMetrics::register(metrics)?,
        );
        let settings = ConnectionSettings::from_config(config);
        Ok(AcceptLoop {
            gate,
            builder: settings.builder(),
            write_idle: settings.download_write_idle,
        })
    }

    /// Accepts on `listener` until `stop` is cancelled, serving every connection with `app` on its own task.
    /// Connections already open when the loop stops are left to finish.
    pub async fn serve(&self, listener: TcpListener, app: Router, stop: CancellationToken) {
        loop {
            let accepted = tokio::select! {
                () = stop.cancelled() => return,
                accepted = listener.accept() => accepted,
            };
            match accepted {
                Ok((stream, peer)) => self.spawn_connection(stream, peer, &app),
                Err(error) => {
                    log_warn!("accept failed", err = &error);
                    if !is_peer_error(&error) {
                        tokio::select! {
                            () = stop.cancelled() => return,
                            () = tokio::time::sleep(ACCEPT_FAULT_BACKOFF) => {}
                        }
                    }
                }
            }
        }
    }

    fn spawn_connection(&self, stream: TcpStream, peer: SocketAddr, app: &Router) {
        let Some(guard) = self.gate.admit() else {
            return;
        };
        tokio::spawn(serve_connection(
            self.builder.clone(),
            guard,
            WriteIdle::new(stream, self.write_idle),
            peer,
            app.clone(),
        ));
    }
}

async fn serve_connection(
    builder: Builder<TokioExecutor>,
    guard: ConnectionGuard,
    stream: WriteIdle<TcpStream>,
    peer: SocketAddr,
    app: Router,
) {
    let _guard = guard;
    let service = WithPeer {
        app: TowerToHyperService::new(app),
        peer: PeerAddr(peer),
    };
    if let Err(error) = builder
        .serve_connection(TokioIo::new(stream), service)
        .await
    {
        log_debug!("connection ended with an error", err = &*error);
    }
}

/// The router, with the TCP peer of the connection added to every request for the client-IP layer.
struct WithPeer {
    app: TowerToHyperService<Router>,
    peer: PeerAddr,
}

impl Service<Request<Incoming>> for WithPeer {
    type Response = <TowerToHyperService<Router> as Service<Request<Incoming>>>::Response;
    type Error = <TowerToHyperService<Router> as Service<Request<Incoming>>>::Error;
    type Future = TowerToHyperServiceFuture<Router, Request<Incoming>>;

    fn call(&self, mut request: Request<Incoming>) -> Self::Future {
        request.extensions_mut().insert(self.peer);
        self.app.call(request)
    }
}

fn is_peer_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> HttpConfig {
        HttpConfig {
            public_url: "https://sunnycloud.test".to_owned(),
            trusted_proxies: Vec::new(),
            public_addr: ([127, 0, 0, 1], 0).into(),
            internal_addr: ([127, 0, 0, 1], 0).into(),
            ops_addr: ([127, 0, 0, 1], 0).into(),
            #[cfg(feature = "test-clock")]
            test_clock_addr: ([127, 0, 0, 1], 0).into(),
            max_connections: 7,
            h1_header_timeout: Duration::from_millis(1_234),
            h2_stream_window: 1_111_111,
            h2_conn_window: 2_222_222,
            h2_max_streams: 33,
            h2_max_pending_reset: 44,
            h2_max_header_list: 5_555,
            h2_keepalive_interval: Duration::from_millis(666),
            h2_keepalive_timeout: Duration::from_millis(777),
            request_timeout: Duration::from_secs(60),
            body_idle_timeout: Duration::from_secs(30),
            download_write_idle: Duration::from_secs(60),
            worker_output_limits: crate::config::WorkerOutputLimits {
                thumb: 5 << 20,
                preview: 20 << 20,
                transcode: 10 << 30,
            },
            shutdown_grace: Duration::from_secs(25),
            min_client_version: None,
            update_url: None,
            server_info: crate::ServerInfo {
                max_lanes: 4,
                batch_max_entries: 500,
                max_staged_per_account: 1000,
                longpoll_max_wait: Duration::from_secs(30),
                min_client_version: String::new(),
            },
            credentials: crate::CredentialFormat {
                session_cookie: "__Host-test".to_owned(),
                device_token_prefix: "dev-".to_owned(),
            },
        }
    }

    #[test]
    fn every_connection_setting_is_taken_from_the_config() {
        // Arrange
        let config = config();

        // Act
        let settings = ConnectionSettings::from_config(&config);

        // Assert
        assert_eq!(
            settings,
            ConnectionSettings {
                h1_header_timeout: Duration::from_millis(1_234),
                h2_stream_window: 1_111_111,
                h2_conn_window: 2_222_222,
                h2_max_streams: 33,
                h2_max_pending_reset: 44,
                h2_max_header_list: 5_555,
                h2_keepalive_interval: Duration::from_millis(666),
                h2_keepalive_timeout: Duration::from_millis(777),
                download_write_idle: Duration::from_secs(60),
            }
        );
    }

    #[test]
    fn a_window_above_the_http2_maximum_is_refused() {
        // Arrange
        let mut config = config();
        config.h2_conn_window = u32::MAX;

        // Act
        let result = config.check();

        // Assert
        assert!(matches!(result, Err(HttpServerError::InvalidConfig(_))));
    }

    #[test]
    fn a_zero_connection_cap_is_refused() {
        // Arrange
        let mut config = config();
        config.max_connections = 0;

        // Act
        let result = config.check();

        // Assert
        assert!(matches!(result, Err(HttpServerError::InvalidConfig(_))));
    }
}
