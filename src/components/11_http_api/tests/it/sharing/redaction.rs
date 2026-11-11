use std::sync::{Arc, Mutex};

use contracts::{AccessGrant, PublicShareError, ShareContent, ShareOpen};
use sc_clock::SystemClock;
use sc_observability::logging::Level;
use sc_observability::{Component, LineSink, LogConfig, LogFormat, subscriber};

use super::support::*;

const TEMPLATE: &str = "/api/v1/public-shares/{token}";

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

#[tokio::test]
async fn the_share_token_reaches_no_log_line_and_no_metric_label_on_any_visitor_route() {
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
    // A second registered dispatcher makes tracing ask each thread's own default (see `metrics_log.rs`).
    let _anchor = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let _logging = tracing::subscriber::set_default(logger);
    let stack = Stack::start().await;
    stack
        .script
        .push(OPEN, Ok::<ShareOpen, PublicShareError>(ShareOpen::NotFound));
    stack.script.push(
        VERIFY,
        Err::<AccessGrant, PublicShareError>(PublicShareError::WrongPassword),
    );
    stack.script.push(
        AUTHORIZE_CONTENT,
        Err::<ShareContent, PublicShareError>(PublicShareError::PasswordRequired),
    );

    // Act
    let answers = [
        stack.as_visitor("GET", &link(""), &[]).await,
        stack
            .call(
                "POST",
                &link("/password-verifications"),
                &[],
                r#"{"password":"nope"}"#,
            )
            .await,
        stack.as_visitor("GET", &link("/content"), &[]).await,
        // Nothing is scripted for these two: the port fails and the failure is logged.
        stack.as_visitor("HEAD", &link("/content"), &[]).await,
        stack.as_visitor("GET", &link("/nowhere"), &[]).await,
    ];
    let scrape = stack.metrics.render().await.expect("render metrics");
    let log = lines.0.lock().expect("the sink is not poisoned").clone();

    // Assert
    let statuses: Vec<u16> = answers.iter().map(|answer| answer.status).collect();
    assert_eq!(statuses, [404, 401, 401, 500, 404]);
    assert!(!scrape.contains(TOKEN), "a metric carries the token");
    assert!(
        scrape.contains(&format!("route=\"{TEMPLATE}\"")),
        "the route is labelled by its template: {scrape}"
    );
    assert!(log.len() >= 5, "every failed answer is logged: {log:?}");
    for line in &log {
        assert!(
            !line.contains(TOKEN),
            "a log line carries the token: {line}"
        );
    }
    assert!(
        log.iter()
            .any(|line| line.contains(&format!("\"route\":\"{TEMPLATE}\""))),
        "{log:?}"
    );
    stack.stop().await;
}
