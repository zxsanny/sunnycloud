//! The scripted fake server: an in-process HTTP/2-over-TLS server for the `/api/v1` routes of `sync-core`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rustls::SupportedProtocolVersion;
use rustls::pki_types::CertificateDer;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

use super::log::{ConnInfo, LoggedRequest, Stats};
use super::script::Script;
use super::server::{Role, Shared, accept_loop};
use super::tls::{self, TlsMode};

pub struct FixtureBuilder {
    script: Script,
    mode: TlsMode,
    flush_grace: Duration,
}

impl FixtureBuilder {
    pub fn tls(mut self, mode: TlsMode) -> Self {
        self.mode = mode;
        self
    }

    /// How long a fault waits before it ends the stream so already-sent bytes reach the wire.
    pub fn flush_grace(mut self, grace: Duration) -> Self {
        self.flush_grace = grace;
        self
    }

    pub async fn start(self) -> Fixture {
        let pki = tls::generate(self.mode);
        let acceptor = TlsAcceptor::from(Arc::clone(&pki.server_config));
        let shared = Arc::new(Shared::new(self.script, self.flush_grace));
        let (api, api_task) = listen(&acceptor, &shared, Role::Api).await;
        let (catcher, catcher_task) = listen(&acceptor, &shared, Role::Catcher).await;
        *shared
            .catcher_origin
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            format!("https://localhost:{}", catcher.port());
        Fixture {
            shared,
            api,
            catcher,
            ca: pki.ca,
            tasks: vec![api_task, catcher_task],
        }
    }
}

async fn listen(
    acceptor: &TlsAcceptor,
    shared: &Arc<Shared>,
    role: Role,
) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind a loopback port");
    let addr = listener.local_addr().expect("bound address");
    let task = tokio::spawn(accept_loop(
        listener,
        acceptor.clone(),
        Arc::clone(shared),
        role,
    ));
    (addr, task)
}

/// One fixture owns its ports, CA, script, log and counters; dropping it stops its tasks.
pub struct Fixture {
    shared: Arc<Shared>,
    api: SocketAddr,
    catcher: SocketAddr,
    ca: CertificateDer<'static>,
    tasks: Vec<JoinHandle<()>>,
}

impl Fixture {
    pub fn builder(script: Script) -> FixtureBuilder {
        FixtureBuilder {
            script,
            mode: TlsMode::Tls13H2,
            flush_grace: Duration::from_millis(25),
        }
    }

    pub async fn start(script: Script) -> Fixture {
        Fixture::builder(script).start().await
    }

    pub fn addr(&self) -> SocketAddr {
        self.api
    }

    pub fn url(&self) -> String {
        format!("https://127.0.0.1:{}", self.api.port())
    }

    /// The redirect target: another host name (`localhost`) and port, same CA.
    pub fn catcher_url(&self) -> String {
        format!("https://localhost:{}", self.catcher.port())
    }

    pub fn catcher_addr(&self) -> SocketAddr {
        self.catcher
    }

    pub fn ca(&self) -> &CertificateDer<'static> {
        &self.ca
    }

    /// A client config that trusts this fixture's CA.
    pub fn client_config(
        &self,
        versions: &[&'static SupportedProtocolVersion],
        alpn: &[&[u8]],
    ) -> Arc<rustls::ClientConfig> {
        tls::client_config(Some(&self.ca), versions, alpn)
    }

    pub fn requests(&self) -> Vec<LoggedRequest> {
        self.shared.requests()
    }

    pub fn catcher_requests(&self) -> Vec<LoggedRequest> {
        self.shared.catcher_requests()
    }

    /// Whether any request, or any `Authorization` header, reached the redirect target.
    pub fn catcher_saw_authorization(&self) -> bool {
        self.catcher_requests()
            .iter()
            .any(|request| request.header("authorization").is_some())
    }

    pub fn connections(&self) -> Vec<ConnInfo> {
        self.shared.stats().connections.clone()
    }

    pub fn with_stats<T>(&self, read: impl FnOnce(&Stats) -> T) -> T {
        read(&self.shared.stats())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
