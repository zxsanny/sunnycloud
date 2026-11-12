//! The TLS side of the end-to-end tests that drive sync-core's real `HttpSyncClient` against the full stack.
//!
//! The client speaks TLS 1.3 and `h2` only, and the server terminates no TLS (a proxy does in production), so a
//! test terminator with a CA generated per run sits between them and copies bytes.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use rcgen::{CertificateParams, Issuer, KeyPair, SanType};
use rustls::ServerConfig;
use rustls::crypto::ring;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sync_core::SyncConfig;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

/// A TLS 1.3 `h2` terminator in front of `upstream`, with the CA its certificate chains to.
pub struct Terminator {
    pub addr: SocketAddr,
    pub ca: CertificateDer<'static>,
    task: JoinHandle<()>,
}

impl Terminator {
    pub async fn start(upstream: SocketAddr) -> Terminator {
        let (ca, config) = certificates();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("terminator binds");
        let addr = listener.local_addr().expect("terminator address");
        let task = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let piped = async {
                        let mut tls = acceptor.accept(client).await?;
                        let mut server = TcpStream::connect(upstream).await?;
                        tokio::io::copy_bidirectional(&mut tls, &mut server).await
                    };
                    if let Err(error) = piped.await {
                        eprintln!("terminator connection ended: {error}");
                    }
                });
            }
        });
        Terminator { addr, ca, task }
    }
}

impl Drop for Terminator {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn certificates() -> (CertificateDer<'static>, ServerConfig) {
    let ca_key = KeyPair::generate().expect("CA key");
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).expect("CA params");
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca = ca_params.self_signed(&ca_key).expect("CA certificate");
    let issuer = Issuer::from_params(&ca_params, &ca_key);
    let leaf_key = KeyPair::generate().expect("leaf key");
    let mut leaf_params = CertificateParams::new(Vec::<String>::new()).expect("leaf params");
    leaf_params.subject_alt_names = vec![SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST))];
    leaf_params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let leaf = leaf_params
        .signed_by(&leaf_key, &issuer)
        .expect("leaf certificate");
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
    let mut config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3")
        .with_no_client_auth()
        .with_single_cert(vec![leaf.der().clone()], key)
        .expect("leaf chain");
    config.alpn_protocols = vec![b"h2".to_vec()];
    (ca.der().clone(), config)
}

/// A client of `family` (`desktop`, `android` or `ios`) for `url`, with short retries.
pub fn client_config(url: String, family: &str) -> SyncConfig {
    SyncConfig {
        server_url: url,
        client_family: family.to_owned(),
        client_version: "1.0.0".to_owned(),
        retry_base: Duration::from_millis(10),
        retry_max: Duration::from_millis(100),
        max_attempts_per_block: 1,
        connect_timeout: Duration::from_secs(2),
        request_idle_timeout: Duration::from_secs(5),
        longpoll_wait: Duration::from_secs(25),
        longpoll_client_margin: Duration::from_secs(1),
        h2_stream_window: 8 * 1024 * 1024,
        h2_conn_window: 32 * 1024 * 1024,
        keepalive_interval: Duration::from_secs(20),
        hash_cpu_share: 25,
        hash_checkpoint_blocks: 32,
        batch_max_bytes_in_flight: 64 * 1024 * 1024,
    }
}
