#![allow(dead_code)]

use std::fmt::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ipc_schema::{DaemonStatus, StatusEvent};
use sunnycloud_settings::daemon_client::{ClientConfig, ConnectionState, DaemonClient};
use sunnycloud_settings_testkit::script::Script;
use sunnycloud_settings_testkit::stub::StubDaemon;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::DefaultGuard;
use tracing::{Event, Level, Metadata, Subscriber};

/// Long enough for a loaded machine, short enough to keep the suite fast.
pub const WAIT: Duration = Duration::from_secs(10);

/// The production client with the timeouts shortened, so no test waits for 5 s.
pub fn config(endpoint: &str) -> ClientConfig {
    let mut config = ClientConfig::new(PathBuf::from(endpoint));
    config.request_timeout = Duration::from_secs(3);
    config.backoff_min = Duration::from_millis(20);
    config.backoff_max = Duration::from_millis(160);
    config
}

pub fn status_with_skipped(count: u64) -> DaemonStatus {
    DaemonStatus {
        server_url: Some("https://cloud.example".to_owned()),
        account: None,
        pairs: Vec::new(),
        skipped_count: count,
        escaped_count: 0,
        holds: Vec::new(),
        update_available: None,
        access_denied: Vec::new(),
        watcher_degraded: Vec::new(),
    }
}

pub async fn wait_connected(client: &DaemonClient) {
    let mut state = client.connection();
    tokio::time::timeout(WAIT, state.wait_for(|s| *s == ConnectionState::Connected))
        .await
        .expect("the client connects in time")
        .expect("the client task is alive");
}

pub struct Session {
    pub stub: StubDaemon,
    pub client: DaemonClient,
    pub events: UnboundedReceiver<StatusEvent>,
}

pub async fn session_with(script: Script, tune: impl FnOnce(&mut ClientConfig)) -> Session {
    let stub = StubDaemon::start(script).await.expect("stub starts");
    let mut config = config(stub.endpoint());
    tune(&mut config);
    let (client, events) = DaemonClient::spawn(config);
    wait_connected(&client).await;
    Session {
        stub,
        client,
        events,
    }
}

pub async fn session(script: Script) -> Session {
    session_with(script, |_| {}).await
}

/// Records every tracing event as `(level, "field=value ...")`.
#[derive(Clone, Default)]
pub struct Captured(Arc<Mutex<Vec<(Level, String)>>>);

impl Captured {
    /// Installs the collector for the current thread until the guard drops; the tests run
    /// the client on that thread (current-thread runtime).
    pub fn install(&self) -> DefaultGuard {
        tracing::subscriber::set_default(Collector(self.clone()))
    }

    pub fn events(&self) -> Vec<(Level, String)> {
        self.0.lock().unwrap().clone()
    }

    pub fn count(&self, level: Level, text: &str) -> usize {
        self.events()
            .iter()
            .filter(|(l, line)| *l == level && line.contains(text))
            .count()
    }
}

struct Collector(Captured);

struct Fields(String);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        write!(self.0, "{}={value:?} ", field.name()).unwrap();
    }
}

impl Subscriber for Collector {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        self.0
            .0
            .lock()
            .unwrap()
            .push((*event.metadata().level(), fields.0));
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}
