use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use oauth2::http::{Method, Request};
use reqwest::Url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::destination::{address_refused, url_refused};
use super::{Lookup, OutboundClient, OutboundError};

/// Answers every request on a fresh local port with `reply` and closes the connection.
async fn local_server(reply: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the test server");
    let address = listener.local_addr().expect("test server address");
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buffer = [0u8; 2048];
                if stream.read(&mut buffer).await.is_err() {
                    return;
                }
                if stream.write_all(reply.as_bytes()).await.is_ok() {
                    stream.shutdown().await.ok();
                }
            });
        }
    });
    address
}

fn table(entries: Vec<(&'static str, Vec<SocketAddr>)>) -> Lookup {
    Arc::new(move |host| {
        let found = entries
            .iter()
            .find(|(name, _)| *name == host)
            .map(|(_, addresses)| addresses.clone());
        Box::pin(
            async move { found.ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound)) },
        )
    })
}

fn get(url: &str) -> Request<Vec<u8>> {
    Request::builder()
        .method(Method::GET)
        .uri(url)
        .body(Vec::new())
        .expect("request")
}

fn ip(text: &str) -> IpAddr {
    text.parse().expect("ip literal")
}

#[test]
fn link_local_and_metadata_addresses_are_refused_and_lan_addresses_are_not() {
    // Arrange
    let refused = [
        "169.254.169.254",
        "169.254.0.1",
        "fe80::1",
        "::ffff:169.254.169.254",
        "fd00:ec2::254",
    ];
    let allowed = [
        "10.0.0.5",
        "192.168.1.20",
        "172.16.0.9",
        "127.0.0.1",
        "::1",
        "93.184.216.34",
        "fd00::1",
    ];

    // Act
    let refused_results = refused.map(|text| address_refused(ip(text)));
    let allowed_results = allowed.map(|text| address_refused(ip(text)));

    // Assert
    assert_eq!(refused_results, [true; 5]);
    assert_eq!(allowed_results, [false; 7]);
}

#[test]
fn urls_are_refused_for_scheme_literal_address_and_metadata_host() {
    // Arrange
    let refused = [
        ("file:///etc/passwd", true),
        ("ftp://idp.test/", true),
        ("http://idp.test/", false),
        ("https://169.254.169.254/latest", true),
        ("https://[fe80::1]/token", true),
        ("https://metadata.google.internal/token", true),
        ("https://METADATA.GOOGLE.INTERNAL./token", true),
    ];
    let allowed = [
        ("https://idp.test/token", false),
        ("http://idp.test/token", true),
        ("https://10.1.2.3:8443/token", false),
    ];

    // Act
    let refused_results: Vec<bool> = refused
        .iter()
        .map(|(url, plain)| url_refused(&Url::parse(url).expect("url"), *plain))
        .collect();
    let allowed_results: Vec<bool> = allowed
        .iter()
        .map(|(url, plain)| url_refused(&Url::parse(url).expect("url"), *plain))
        .collect();

    // Assert
    assert_eq!(refused_results, vec![true; 7]);
    assert_eq!(allowed_results, vec![false; 3]);
}

#[tokio::test]
async fn a_name_that_resolves_to_a_metadata_address_is_refused_before_connecting() {
    // Arrange
    let metadata = SocketAddr::new(ip("169.254.169.254"), 80);
    let client = OutboundClient::with_lookup(true, table(vec![("rebind.test", vec![metadata])]))
        .expect("client");

    // Act
    let started = std::time::Instant::now();
    let result = client.send(get("http://rebind.test/token")).await;

    // Assert
    assert!(matches!(result, Err(OutboundError::Refused)), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn a_name_with_a_refused_and_a_lan_address_connects_to_the_lan_address_only() {
    // Arrange
    let server =
        local_server("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").await;
    let metadata = SocketAddr::new(ip("169.254.169.254"), 80);
    let client = OutboundClient::with_lookup(
        true,
        table(vec![(
            "mixed.test",
            vec![metadata, SocketAddr::new(server.ip(), 0)],
        )]),
    )
    .expect("client");

    // Act
    let response = client
        .send(get(&format!("http://mixed.test:{}/", server.port())))
        .await
        .expect("answer from the LAN address");

    // Assert
    assert_eq!(response.body(), b"ok");
}

#[tokio::test]
async fn a_redirect_answer_is_an_error_and_is_not_followed() {
    // Arrange
    let canary =
        local_server("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
    let redirecting = Box::leak(
        format!("HTTP/1.1 302 Found\r\nLocation: http://{canary}/canary\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .into_boxed_str(),
    );
    let server = local_server(redirecting).await;
    let client = OutboundClient::new(true).expect("client");

    // Act
    let result = client.send(get(&format!("http://{server}/"))).await;

    // Assert
    assert!(matches!(result, Err(OutboundError::Redirect)), "{result:?}");
}

#[tokio::test]
async fn a_body_over_the_limit_is_refused() {
    // Arrange
    let oversized = Box::leak(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            super::MAX_BODY_BYTES + 1,
            "x".repeat(super::MAX_BODY_BYTES + 1)
        )
        .into_boxed_str(),
    );
    let server = local_server(oversized).await;
    let client = OutboundClient::new(true).expect("client");

    // Act
    let result = client.send(get(&format!("http://{server}/"))).await;

    // Assert
    assert!(matches!(result, Err(OutboundError::TooLarge)), "{result:?}");
}
