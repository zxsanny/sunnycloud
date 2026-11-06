//! AC-4 of AZ-3970: the request log and the redirect catcher.

mod support;

use bytes::Bytes;
use support::{Answer, Client, Fixture, Route, Script};

const TOKEN: &str = "Bearer test-token-not-a-secret";

fn redirecting_script() -> Script {
    Script::new().on(
        Route::RootNode,
        Answer::redirect("{catcher}/api/v1/nodes/root"),
    )
}

async fn connect(addr: std::net::SocketAddr, name: &str, fixture: &Fixture) -> Client {
    let config = fixture.client_config(&[&rustls::version::TLS13], &[b"h2"]);
    Client::connect(addr, name, config)
        .await
        .expect("handshake with the fixture")
}

#[tokio::test]
async fn a_client_that_stays_on_its_host_never_reaches_the_catcher() {
    let fixture = Fixture::start(redirecting_script()).await;
    let mut client = connect(fixture.addr(), "127.0.0.1", &fixture).await;

    let reply = client
        .send(
            "GET",
            "/api/v1/nodes/root",
            &[
                ("authorization", TOKEN),
                ("x-sunnycloud-client", "desktop/1.0.0"),
            ],
            Bytes::new(),
        )
        .await
        .expect("the redirect answer");

    assert_eq!(reply.status, 302);
    assert_eq!(
        reply.header("location"),
        Some(format!("{}/api/v1/nodes/root", fixture.catcher_url()).as_str())
    );
    let log = fixture.requests();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].method, "GET");
    assert_eq!(log[0].header("authorization"), Some(TOKEN));
    assert_eq!(log[0].header("x-sunnycloud-client"), Some("desktop/1.0.0"));
    assert!(fixture.catcher_requests().is_empty());
    assert!(!fixture.catcher_saw_authorization());
}

#[tokio::test]
async fn a_client_that_follows_the_redirect_is_caught_with_its_credentials() {
    let fixture = Fixture::start(redirecting_script()).await;
    let mut client = connect(fixture.addr(), "127.0.0.1", &fixture).await;
    let headers = [("authorization", TOKEN)];
    let redirect = client
        .send("GET", "/api/v1/nodes/root", &headers, Bytes::new())
        .await
        .expect("the redirect answer");
    assert_eq!(redirect.status, 302);

    let mut naive = connect(fixture.catcher_addr(), "localhost", &fixture).await;
    let followed = naive
        .send("GET", "/api/v1/nodes/root", &headers, Bytes::new())
        .await
        .expect("the catcher answer");

    assert_eq!(followed.status, 200);
    let caught = fixture.catcher_requests();
    assert_eq!(caught.len(), 1);
    assert_eq!(caught[0].target, "/api/v1/nodes/root");
    assert_eq!(caught[0].header("authorization"), Some(TOKEN));
    assert!(fixture.catcher_saw_authorization());
    assert_eq!(
        fixture.requests().len(),
        1,
        "the catcher does not write to the main log"
    );
}
