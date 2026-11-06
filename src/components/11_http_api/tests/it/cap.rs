use std::time::Duration;

use sunnycloud_http_api::HttpConfig;

use crate::harness::clients::{get_request, h1_connect};
use crate::harness::{EVENT_BOUND, Server, default_config, metric, wait_until};

const REJECTED: &str = r#"sunnycloud_connections_rejected_total{reason="limit"}"#;
const OPEN: &str = "sunnycloud_connections_open";

#[tokio::test]
async fn connections_above_the_cap_are_closed_and_counted_while_the_first_fifty_keep_working() {
    // Arrange
    let config = HttpConfig {
        max_connections: 50,
        h1_header_timeout: Duration::from_secs(60),
        ..default_config()
    };
    let server = Server::start(config).await;
    let mut admitted = Vec::new();
    for _ in 0..50 {
        let mut sender = h1_connect(server.public).await;
        let response = sender
            .send_request(get_request("/"))
            .await
            .expect("a connection under the cap is served");
        assert_eq!(response.status(), 404);
        admitted.push(sender);
    }

    // Act
    let mut refused = 0;
    for _ in 0..10 {
        let mut sender = h1_connect(server.public).await;
        let outcome = tokio::time::timeout(EVENT_BOUND, sender.send_request(get_request("/")))
            .await
            .expect("the server answers or closes");
        if outcome.is_err() {
            refused += 1;
        }
    }
    let mut still_served = 0;
    for sender in &mut admitted {
        let response = sender
            .send_request(get_request("/"))
            .await
            .expect("an admitted connection keeps working");
        if response.status() == 404 {
            still_served += 1;
        }
    }

    // Assert
    assert_eq!(refused, 10);
    assert_eq!(still_served, 50);
    assert_eq!(metric(&server.metrics, REJECTED).await, Some(10.0));
    assert_eq!(metric(&server.metrics, OPEN).await, Some(50.0));
    drop(admitted);
    wait_until("the open gauge to fall to 0", || async {
        metric(&server.metrics, OPEN).await == Some(0.0)
    })
    .await;
    server.stop().await.expect("run returns");
}
