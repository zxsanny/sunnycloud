//! The TLS client policy: TLS 1.3 only, ALPN `h2` only, certificates checked by the platform verifier.

use std::sync::Arc;

use rustls::ClientConfig;
use rustls::crypto::ring;
use rustls::pki_types::CertificateDer;
use rustls_platform_verifier::Verifier;

/// `extra_roots` are trusted in addition to the system store; the release path passes none.
pub(crate) fn client_config(
    extra_roots: Vec<CertificateDer<'static>>,
) -> Result<ClientConfig, rustls::Error> {
    let provider = Arc::new(ring::default_provider());
    let verifier = verifier(extra_roots, Arc::clone(&provider))?;
    // `dangerous()` is rustls's name for installing any verifier; this one is the platform verifier.
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];
    Ok(config)
}

#[cfg(not(target_os = "android"))]
fn verifier(
    extra_roots: Vec<CertificateDer<'static>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
) -> Result<Verifier, rustls::Error> {
    Verifier::new_with_extra_roots(extra_roots, provider)
}

/// The Android verifier takes no extra roots; only the test entry point passes any, and it is not built there.
#[cfg(target_os = "android")]
fn verifier(
    _extra_roots: Vec<CertificateDer<'static>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
) -> Result<Verifier, rustls::Error> {
    Verifier::new(provider)
}
