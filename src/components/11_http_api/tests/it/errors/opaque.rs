use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use sc_clock::SystemClock;
use sc_observability::__private::tracing;
use sc_observability::logging::Level;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};
use serde_json::Value;

use super::cases::PLANTED;
use super::probe::{ErrorProbe, fresh_id};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<String>>>);

impl Capture {
    fn lines(&self) -> Vec<Value> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|line| serde_json::from_str(line).expect("one JSON object per line"))
            .collect()
    }
}

impl LineSink for Capture {
    fn write_line(&self, line: &str) -> io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_owned());
        Ok(())
    }
}

#[tokio::test]
async fn an_unexpected_error_gives_an_opaque_500_and_exactly_one_error_line_with_the_chain() {
    // Arrange
    let capture = Capture::default();
    let config = LogConfig {
        format: LogFormat::Json,
        level: Level::INFO,
        component: Component::Api,
    };
    let (logger, _stats) = subscriber(
        config,
        Arc::new(SystemClock::new()),
        Arc::new(capture.clone()),
    );
    let _installed = tracing::subscriber::set_default(logger);
    let probe = ErrorProbe::start().await;
    let missing = probe.get("/probe/errors/not_found", fresh_id()).await;
    let id = fresh_id();

    // Act
    let reply = probe.get("/probe/errors/internal", id).await;

    // Assert
    assert_eq!(missing.status.as_u16(), 404);
    assert_eq!(reply.status.as_u16(), 500);
    let body = String::from_utf8(reply.body.clone()).expect("utf-8 body");
    for marker in PLANTED {
        assert!(!body.contains(marker), "the body leaks {marker}: {body}");
    }
    let body = reply.json();
    assert_eq!(body["code"], "internal");
    assert!(body.get("detail").is_none());
    let errors: Vec<Value> = capture
        .lines()
        .into_iter()
        .filter(|line| line["level"] == "error")
        .collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["request_id"], id.to_string());
    let chain = errors[0]["err"].as_str().expect("the chain is logged");
    for marker in PLANTED {
        assert!(chain.contains(marker), "the log lacks {marker}: {chain}");
    }
    probe.stop().await;
}
