//! AC-4: no daemon, then a daemon; the client reconnects on its own with a backoff.

mod common;

use std::time::{Duration, Instant};

use sunnycloud_settings::daemon_client::{ClientError, ConnectionState, DaemonClient};
use sunnycloud_settings_testkit::script::Script;
use sunnycloud_settings_testkit::stub::StubDaemon;

use common::{WAIT, config, wait_connected};

/// With the test config: 20 ms doubling to at most 160 ms.
const SCHEDULE_MS: [u64; 4] = [20, 40, 80, 160];

/// A running stub whose endpoint does not exist until `reveal`.
#[cfg(unix)]
struct HiddenStub {
    stub: StubDaemon,
    visible: std::path::PathBuf,
    hidden: std::path::PathBuf,
}

#[cfg(unix)]
impl HiddenStub {
    async fn new() -> Self {
        let stub = StubDaemon::start(Script::default()).await.unwrap();
        let visible = stub.runtime_dir().to_path_buf();
        let hidden = visible.with_file_name("hidden-by-the-test");
        std::fs::rename(&visible, &hidden).unwrap();
        Self {
            stub,
            visible,
            hidden,
        }
    }

    fn endpoint(&self) -> String {
        self.stub.endpoint().to_owned()
    }

    async fn reveal(&self) {
        std::fs::rename(&self.hidden, &self.visible).unwrap();
    }
}

#[cfg(windows)]
struct HiddenStub {
    pipe: String,
    stub: std::cell::RefCell<Option<StubDaemon>>,
}

#[cfg(windows)]
impl HiddenStub {
    async fn new() -> Self {
        Self {
            pipe: format!(r"\\.\pipe\sunnycloud-test-reconnect-{}", std::process::id()),
            stub: std::cell::RefCell::new(None),
        }
    }

    fn endpoint(&self) -> String {
        self.pipe.clone()
    }

    async fn reveal(&self) {
        let stub = StubDaemon::start_on_pipe(Script::default(), self.pipe.clone())
            .await
            .unwrap();
        *self.stub.borrow_mut() = Some(stub);
    }
}

#[tokio::test]
async fn an_absent_daemon_is_reported_and_retried_with_a_growing_backoff_until_it_appears() {
    let hidden = HiddenStub::new().await;
    let (client, _events) = DaemonClient::spawn(config(&hidden.endpoint()));
    let mut state = client.connection();

    let mut attempts: Vec<Instant> = Vec::new();
    while attempts.len() < SCHEDULE_MS.len() + 1 {
        tokio::time::timeout(WAIT, state.changed())
            .await
            .expect("a new attempt is reported")
            .unwrap();
        match &*state.borrow() {
            ConnectionState::Down { attempt, cause } => {
                assert_eq!(*cause, ClientError::DaemonNotRunning);
                assert_eq!(*attempt as usize, attempts.len() + 1);
                attempts.push(Instant::now());
            }
            other => panic!("unexpected state {other:?}"),
        }
    }
    let during_outage = client.status().await;

    let gaps: Vec<Duration> = attempts.windows(2).map(|w| w[1] - w[0]).collect();
    for (gap, scheduled) in gaps.iter().zip(SCHEDULE_MS) {
        let scheduled = Duration::from_millis(scheduled);
        assert!(
            *gap >= scheduled && *gap < scheduled + Duration::from_millis(150),
            "gap {gap:?}, scheduled {scheduled:?}; all gaps {gaps:?}"
        );
    }
    assert_eq!(during_outage, Err(ClientError::DaemonNotRunning));

    hidden.reveal().await;
    wait_connected(&client).await;
    assert!(
        client.status().await.is_ok(),
        "status after the daemon appeared"
    );
}
