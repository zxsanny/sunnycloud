use std::time::Duration;

use hyper::StatusCode;
use prometheus_parse::{Scrape, Value};
use sunnycloud_http_api::{DurationLabels, HttpConfig, MethodLabel, RequestLabels, StatusClass};

use crate::harness::clients::{get_request, h1_connect, h1_get};
use crate::harness::{EVENT_BOUND, Server, default_config, metric, wait_until};

const NODE_ROUTE: &str = "/api/v1/nodes/{id}";
const REQUESTS: &str = "sunnycloud_http_requests_total";
const DURATION: &str = "sunnycloud_http_request_duration_seconds";

/// The scrape as text and as `prometheus_parse` reads it. That parser drops a sample whose label value holds
/// `}` (a route template), so the HTTP series are asserted on the text.
async fn scrape(server: &Server) -> (String, Scrape) {
    let (status, text) = h1_get(server.ops, "/metrics").await;
    assert_eq!(status, StatusCode::OK);
    let parsed = Scrape::parse(text.lines().map(|line| Ok(line.to_owned())))
        .expect("Prometheus text parses");
    (text, parsed)
}

fn sample_names(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| line.split(['{', ' ']).next().unwrap_or(line))
        .collect()
}

#[tokio::test]
async fn metrics_export_the_five_http_families_with_one_prefix_and_route_template_series() {
    // Arrange
    let config = HttpConfig {
        max_connections: 5,
        h1_header_timeout: Duration::from_secs(60),
        ..default_config()
    };
    let server = Server::start(config).await;
    let mut held = Vec::new();
    for _ in 0..5 {
        let mut sender = h1_connect(server.public).await;
        sender
            .send_request(get_request("/"))
            .await
            .expect("a connection under the cap is served");
        held.push(sender);
    }
    for _ in 0..10 {
        let mut sender = h1_connect(server.public).await;
        let outcome = tokio::time::timeout(EVENT_BOUND, sender.send_request(get_request("/")))
            .await
            .expect("the server answers or closes");
        assert!(outcome.is_err(), "a connection above the cap is closed");
    }
    drop(held);
    wait_until("the held connections to close", || async {
        metric(&server.metrics, "sunnycloud_connections_open").await == Some(0.0)
    })
    .await;
    // The request layers of E-21/c05 increment these families; the handles are the ones the server registered.
    let labels = RequestLabels {
        route: NODE_ROUTE,
        method: MethodLabel::Get,
        status: StatusClass::Success,
    };
    let duration = DurationLabels { route: NODE_ROUTE };
    for _ in 0..1000 {
        server.http_metrics.requests.inc(labels);
        server
            .http_metrics
            .request_duration
            .observe(duration, 0.002);
    }

    // Act
    let (text, parsed) = scrape(&server).await;

    // Assert
    for name in sample_names(&text) {
        assert!(name.starts_with("sunnycloud_"), "{name} has the prefix");
        assert!(!name["sunnycloud_".len()..].starts_with("sunnycloud_"));
    }
    let value_of = |family: &str| {
        parsed
            .samples
            .iter()
            .find(|sample| sample.metric == family)
            .map(|sample| sample.value.clone())
    };
    assert_eq!(
        value_of("sunnycloud_connections_rejected_total"),
        Some(Value::Counter(10.0))
    );
    assert_eq!(
        value_of("sunnycloud_connections_open"),
        Some(Value::Gauge(1.0)),
        "the scraping connection"
    );
    assert_eq!(
        value_of("sunnycloud_body_limit_exceeded_total"),
        Some(Value::Counter(0.0))
    );
    let request_series: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with(REQUESTS) && line.contains(NODE_ROUTE))
        .collect();
    assert_eq!(
        request_series,
        [format!(
            "{REQUESTS}{{method=\"GET\",route=\"{NODE_ROUTE}\",status_class=\"2xx\"}} 1000"
        )]
    );
    let duration_count = format!("{DURATION}_count{{route=\"{NODE_ROUTE}\"}} 1000");
    assert!(text.lines().any(|line| line == duration_count));
    for line in text
        .lines()
        .filter(|line| line.starts_with(REQUESTS) || line.starts_with(DURATION))
    {
        assert!(
            line.contains("route=\""),
            "the route template labels the HTTP families: {line}"
        );
    }
    server.stop().await.expect("run returns");
}
