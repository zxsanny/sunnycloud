use std::sync::{Arc, Mutex};

use sc_clock::SystemClock;
use sc_observability::logging::Level;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};

use super::client::H2;
use super::stack::{NODE_ROUTE, SHARE_ROUTE, Stack};

const REQUESTS: &str = "sunnycloud_http_requests_total";
const DURATION: &str = "sunnycloud_http_request_duration_seconds";
const SHARE_TOKEN: &str = "SECRETSHARETOKEN9d41";

#[derive(Default)]
struct Lines(Mutex<Vec<String>>);

impl LineSink for Lines {
    fn write_line(&self, line: &str) -> std::io::Result<()> {
        self.0
            .lock()
            .expect("the sink is not poisoned")
            .push(line.to_owned());
        Ok(())
    }
}

impl Lines {
    fn all(&self) -> Vec<String> {
        self.0.lock().expect("the sink is not poisoned").clone()
    }
}

#[tokio::test]
async fn ids_make_one_series_per_template_and_only_failures_and_never_a_token_reach_the_log() {
    // Arrange
    let lines = Arc::new(Lines::default());
    let (logger, _stats) = subscriber(
        LogConfig {
            format: LogFormat::Json,
            level: Level::INFO,
            component: Component::Api,
        },
        Arc::new(SystemClock::new()),
        lines.clone(),
    );
    // `Dispatch::new` registers a dispatcher with tracing. With only one registered, tracing decides each call
    // site from the default of the thread that reaches it first, and concurrent tests reach these call sites
    // with none, which disables them for this test. A second registered dispatcher makes tracing ask every
    // thread's own default instead.
    let _anchor = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let _logging = tracing::subscriber::set_default(logger);
    let stack = Stack::start(&[]).await;
    let mut connection = H2::connect(stack.addr).await;

    // Act
    for index in 0..1000 {
        let reply = connection
            .get(&format!("/api/v1/nodes/node-{index:04}"))
            .await;
        assert_eq!(reply.status.as_u16(), 200);
    }
    let mut failed_ids = Vec::new();
    for _ in 0..10 {
        let reply = connection.get("/api/v1/nodes/missing").await;
        assert_eq!(reply.status.as_u16(), 404);
        failed_ids.push(reply.request_id().to_owned());
    }
    let share = connection
        .get(&format!("/api/v1/public-shares/{SHARE_TOKEN}"))
        .await;
    let unrouted_share = connection
        .get(&format!("/api/v1/public-shares/{SHARE_TOKEN}/unknown"))
        .await;
    let requests = stack.series(REQUESTS).await;
    let durations = stack.series(DURATION).await;
    let log = lines.all();

    // Assert
    assert_eq!(share.status.as_u16(), 200);
    assert_eq!(unrouted_share.status.as_u16(), 404);
    let count = |route: &str, method: &str, class: &str| {
        let series =
            format!("{REQUESTS}{{method=\"{method}\",route=\"{route}\",status_class=\"{class}\"}}");
        requests
            .iter()
            .find_map(|line| line.strip_prefix(&series))
            .map(|value| value.trim().to_owned())
    };
    assert_eq!(count(NODE_ROUTE, "GET", "2xx").as_deref(), Some("1000"));
    assert_eq!(count(NODE_ROUTE, "GET", "4xx").as_deref(), Some("10"));
    assert_eq!(count(SHARE_ROUTE, "GET", "2xx").as_deref(), Some("1"));
    assert_eq!(count("unmatched", "GET", "4xx").as_deref(), Some("1"));
    assert_eq!(
        requests.len(),
        4,
        "one series per template and class: {requests:?}"
    );
    for line in requests.iter().chain(&durations) {
        assert!(!line.contains("node-0"), "{line}");
        assert!(!line.contains(SHARE_TOKEN), "{line}");
        assert!(!line.contains("missing"), "{line}");
    }
    assert!(
        durations
            .iter()
            .any(|line| line
                .starts_with(&format!("{DURATION}_count{{route=\"{NODE_ROUTE}\"}} 1010")))
    );
    let answered: Vec<&String> = log
        .iter()
        .filter(|line| line.contains("\"msg\":\"request answered\""))
        .collect();
    assert_eq!(
        answered.len(),
        11,
        "10 failures and one unrouted share: {log:?}"
    );
    assert_eq!(log.len(), 11, "no line for the 2xx answers: {log:?}");
    assert!(
        log.iter()
            .all(|line| !line.contains(SHARE_TOKEN) && !line.contains("node-0"))
    );
    for id in &failed_ids {
        let line = answered
            .iter()
            .find(|line| line.contains(id.as_str()))
            .expect("the failing request is logged under its id");
        assert!(
            line.contains(&format!("\"route\":\"{NODE_ROUTE}\"")),
            "{line}"
        );
        assert!(line.contains("\"status\":404"), "{line}");
    }
    let unrouted = answered
        .iter()
        .filter(|line| !line.contains("\"route\""))
        .count();
    assert_eq!(
        unrouted, 1,
        "the unmatched answer is logged without a route"
    );
    stack.stop().await;
}
