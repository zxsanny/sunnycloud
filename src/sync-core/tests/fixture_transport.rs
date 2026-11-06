//! AC-3 of AZ-3970: the three transport modes, and a test CA that only this fixture's clients trust.

mod support;

use rustls::ProtocolVersion;
use support::{Answer, Client, Fixture, Route, Script, TlsMode, client_config};

fn script() -> Script {
    Script::new().always(Route::ServerInfo, Answer::no_content())
}

async fn start(mode: TlsMode) -> Fixture {
    Fixture::builder(script()).tls(mode).start().await
}

async fn connect(
    fixture: &Fixture,
    versions: &[&'static rustls::SupportedProtocolVersion],
    alpn: &[&[u8]],
) -> Result<Client, String> {
    Client::connect(
        fixture.addr(),
        "127.0.0.1",
        fixture.client_config(versions, alpn),
    )
    .await
}

const TLS13: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];
const TLS12: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS12];

#[tokio::test]
async fn tls13_with_h2_is_the_default_mode() {
    let fixture = Fixture::start(script()).await;

    let mut client = connect(&fixture, TLS13, &[b"h2"]).await.expect("handshake");
    let reply = client
        .get("/api/v1/server-info")
        .await
        .expect("request over h2");

    assert_eq!(reply.status, 204);
    assert_eq!(client.tls_version, Some(ProtocolVersion::TLSv1_3));
    assert_eq!(client.alpn.as_deref(), Some("h2"));
    let negotiated = fixture.connections();
    assert_eq!(negotiated.len(), 1);
    assert_eq!(negotiated[0].tls_version, Some(ProtocolVersion::TLSv1_3));
    assert_eq!(negotiated[0].alpn.as_deref(), Some("h2"));
}

#[tokio::test]
async fn tls12_only_mode_refuses_a_tls13_only_client() {
    let fixture = start(TlsMode::Tls12Only).await;

    let refused = connect(&fixture, TLS13, &[b"h2"]).await;
    let mut accepted = connect(&fixture, TLS12, &[b"h2", b"http/1.1"])
        .await
        .expect("TLS 1.2 handshake");
    let reply = accepted
        .get("/api/v1/server-info")
        .await
        .expect("request over TLS 1.2");

    assert!(refused.is_err(), "a TLS 1.3-only client must not connect");
    assert_eq!(accepted.tls_version, Some(ProtocolVersion::TLSv1_2));
    assert_eq!(reply.status, 204);
    assert_eq!(fixture.with_stats(|s| s.handshake_failures.len()), 1);
}

#[tokio::test]
async fn tls13_without_h2_refuses_an_h2_only_client_and_serves_http1() {
    let fixture = start(TlsMode::Tls13NoH2).await;

    let refused = connect(&fixture, TLS13, &[b"h2"]).await;
    let mut accepted = connect(&fixture, TLS13, &[b"h2", b"http/1.1"])
        .await
        .expect("handshake");
    let reply = accepted
        .get("/api/v1/server-info")
        .await
        .expect("request over HTTP/1.1");

    assert!(refused.is_err(), "an h2-only client must not connect");
    assert_eq!(accepted.alpn.as_deref(), Some("http/1.1"));
    assert_eq!(accepted.tls_version, Some(ProtocolVersion::TLSv1_3));
    assert_eq!(reply.status, 204);
    assert_eq!(fixture.with_stats(|s| s.handshake_failures.len()), 1);
}

#[tokio::test]
async fn a_client_without_the_test_ca_is_refused() {
    let fixture = Fixture::start(script()).await;
    let other = Fixture::start(script()).await;

    let no_roots = client_config(None, TLS13, &[b"h2"]);
    let foreign_ca = client_config(Some(other.ca()), TLS13, &[b"h2"]);
    let refused_empty = Client::connect(fixture.addr(), "127.0.0.1", no_roots).await;
    let refused_foreign = Client::connect(fixture.addr(), "127.0.0.1", foreign_ca).await;

    for refused in [refused_empty, refused_foreign] {
        let error = refused.err().expect("the certificate is not trusted");
        assert!(
            error.contains("invalid peer certificate"),
            "unexpected error: {error}"
        );
    }
}

#[tokio::test]
async fn the_certificate_does_not_vouch_for_other_names() {
    let fixture = Fixture::start(script()).await;

    let wrong_name = Client::connect(
        fixture.addr(),
        "evil.example",
        fixture.client_config(TLS13, &[b"h2"]),
    )
    .await;

    let error = wrong_name.err().expect("name mismatch is refused");
    assert!(
        error.contains("not valid for name"),
        "unexpected error: {error}"
    );
}
