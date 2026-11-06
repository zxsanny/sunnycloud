//! The fixture's test CA and server certificate, generated at runtime for each fixture.
//! Nothing here is compiled outside the test targets, and no key or certificate is stored in the source.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SanType,
};
use rustls::crypto::{CryptoProvider, ring};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig, SupportedProtocolVersion, version};

/// The three transport modes of the fixture (`AC-3`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsMode {
    /// TLS 1.3, ALPN `h2`.
    Tls13H2,
    /// TLS 1.2 only, ALPN `h2` and `http/1.1`.
    Tls12Only,
    /// TLS 1.3, ALPN `http/1.1` only.
    Tls13NoH2,
}

pub struct Pki {
    pub ca: CertificateDer<'static>,
    pub server_config: Arc<ServerConfig>,
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(ring::default_provider())
}

/// A fresh CA and a server certificate for `localhost` and `127.0.0.1` signed by it.
pub fn generate(mode: TlsMode) -> Pki {
    let ca_key = KeyPair::generate().expect("CA key");
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).expect("CA params");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "sunnycloud scripted fixture CA");
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca = ca_params.self_signed(&ca_key).expect("CA certificate");
    let issuer = Issuer::from_params(&ca_params, &ca_key);

    let leaf_key = KeyPair::generate().expect("server key");
    let mut leaf_params =
        CertificateParams::new(vec!["localhost".to_owned()]).expect("leaf params");
    leaf_params
        .subject_alt_names
        .push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    leaf_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let leaf = leaf_params
        .signed_by(&leaf_key, &issuer)
        .expect("server certificate");

    let (versions, alpn): (&[&SupportedProtocolVersion], Vec<Vec<u8>>) = match mode {
        TlsMode::Tls13H2 => (&[&version::TLS13], vec![b"h2".to_vec()]),
        TlsMode::Tls12Only => (
            &[&version::TLS12],
            vec![b"h2".to_vec(), b"http/1.1".to_vec()],
        ),
        TlsMode::Tls13NoH2 => (&[&version::TLS13], vec![b"http/1.1".to_vec()]),
    };
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
    let mut server_config = ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(versions)
        .expect("server protocol versions")
        .with_no_client_auth()
        .with_single_cert(vec![leaf.der().clone()], key)
        .expect("server certificate chain");
    server_config.alpn_protocols = alpn;
    Pki {
        ca: ca.der().clone(),
        server_config: Arc::new(server_config),
    }
}

/// A client config that trusts `trusted` (nothing when `None`), speaks `versions` and offers `alpn`.
pub fn client_config(
    trusted: Option<&CertificateDer<'static>>,
    versions: &[&'static SupportedProtocolVersion],
    alpn: &[&[u8]],
) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    if let Some(ca) = trusted {
        roots.add(ca.clone()).expect("fixture CA is a valid root");
    }
    let mut config = ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(versions)
        .expect("client protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    Arc::new(config)
}
