mod common;

use std::sync::{Arc, Mutex};

use common::*;
use ipc_schema::*;
use serde_json::json;

#[derive(Default)]
struct CollectingSink {
    events: Mutex<Vec<StatusEvent>>,
}

impl StatusSink for CollectingSink {
    fn send(&self, event: StatusEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// Answers every method with a fixed value and records the order of calls. `pause` moves
/// pair 1 to `Paused` and notifies the subscriber, as the daemon must.
struct FakeDaemon {
    calls: Mutex<Vec<&'static str>>,
    sink: Mutex<Option<Arc<dyn StatusSink>>>,
    status: Mutex<DaemonStatus>,
}

impl FakeDaemon {
    fn new() -> Self {
        let mut status = full_status();
        status.pairs[0].state = PairState::Active;
        Self {
            calls: Mutex::default(),
            sink: Mutex::default(),
            status: Mutex::new(status),
        }
    }

    fn record(&self, name: &'static str) {
        self.calls.lock().unwrap().push(name);
    }
}

impl DaemonControl for FakeDaemon {
    async fn status(&self) -> ControlResult<DaemonStatus> {
        self.record("status");
        Ok(self.status.lock().unwrap().clone())
    }
    async fn set_server(&self, params: SetServerParams) -> ControlResult<()> {
        self.record("set_server");
        if params.url.starts_with("https://") {
            Ok(())
        } else {
            Err(ControlError::HttpsRequired)
        }
    }
    async fn list_server_folders(&self, _: ListServerFoldersParams) -> ControlResult<FolderPage> {
        self.record("list_server_folders");
        Ok(FolderPage {
            items: Vec::new(),
            next: None,
        })
    }
    async fn list_pairs(&self) -> ControlResult<Vec<PairView>> {
        self.record("list_pairs");
        Ok(self.status.lock().unwrap().pairs.clone())
    }
    async fn add_pair(&self, _: PairSpec) -> ControlResult<PairId> {
        self.record("add_pair");
        Ok(PairId(7))
    }
    async fn update_pair(&self, _: UpdatePairParams) -> ControlResult<()> {
        self.record("update_pair");
        Err(ControlError::RulesInvalid { line: 3 })
    }
    async fn remove_pair(&self, _: PairIdParams) -> ControlResult<()> {
        self.record("remove_pair");
        Ok(())
    }
    async fn pause(&self, params: PairIdParams) -> ControlResult<()> {
        self.record("pause");
        if params.pair_id != PairId(1) {
            return Err(ControlError::UnknownPair);
        }
        let snapshot = {
            let mut status = self.status.lock().unwrap();
            status.pairs[0].state = PairState::Paused;
            status.clone()
        };
        if let Some(sink) = self.sink.lock().unwrap().as_ref() {
            sink.send(StatusEvent { status: snapshot });
        }
        Ok(())
    }
    async fn resume(&self, _: PairIdParams) -> ControlResult<()> {
        self.record("resume");
        Ok(())
    }
    async fn list_skipped(&self, _: ListEntriesParams) -> ControlResult<SkippedPage> {
        self.record("list_skipped");
        Ok(SkippedPage {
            items: Vec::new(),
            next: None,
        })
    }
    async fn list_escaped(&self, _: ListEntriesParams) -> ControlResult<EscapedPage> {
        self.record("list_escaped");
        Ok(EscapedPage {
            items: Vec::new(),
            next: None,
        })
    }
    async fn get_settings(&self) -> ControlResult<Settings> {
        self.record("get_settings");
        Ok(settings())
    }
    async fn set_settings(&self, _: SettingsPatch) -> ControlResult<Settings> {
        self.record("set_settings");
        Ok(settings())
    }
    async fn resolve_hold(&self, _: ResolveHoldParams) -> ControlResult<()> {
        self.record("resolve_hold");
        Err(ControlError::NoSuchHold)
    }
    async fn login_start(&self, _: LoginStartParams) -> ControlResult<LoginPrompt> {
        self.record("login_start");
        Ok(LoginPrompt {
            verification_url: "https://cloud.example.org/device".to_owned(),
            user_code: "WDJB-MJHT".to_owned(),
            expires_at: "2026-11-05T10:00:00Z".to_owned(),
        })
    }
    async fn login_status(&self) -> ControlResult<LoginState> {
        self.record("login_status");
        Ok(LoginState::Pending)
    }
    async fn logout(&self) -> ControlResult<LogoutOutcome> {
        self.record("logout");
        Ok(LogoutOutcome {
            server_revoked: false,
        })
    }
    async fn subscribe(&self, sink: Arc<dyn StatusSink>) -> ControlResult<()> {
        self.record("subscribe");
        *self.sink.lock().unwrap() = Some(sink);
        Ok(())
    }
}

fn call(
    daemon: &FakeDaemon,
    sink: &Arc<dyn StatusSink>,
    method: &str,
    params: serde_json::Value,
) -> Response {
    let line =
        serde_json::to_vec(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))
            .unwrap();
    let decoded = decode_call(&line).expect("decodes");
    block_on(dispatch(daemon, sink, decoded.id, decoded.call))
}

fn sink() -> (Arc<CollectingSink>, Arc<dyn StatusSink>) {
    let collecting = Arc::new(CollectingSink::default());
    let as_trait: Arc<dyn StatusSink> = collecting.clone();
    (collecting, as_trait)
}

fn sample_params(method: &str) -> serde_json::Value {
    match method {
        "set_server" => json!({"url": "https://x.example"}),
        "update_pair" => json!({"pair_id": 1, "patch": {"rules_text": "a"}}),
        "remove_pair" | "pause" | "resume" => json!({"pair_id": 1}),
        "list_skipped" | "list_escaped" => json!({"pair_id": null, "page": null}),
        "resolve_hold" => json!({"pair_id": 1, "decision": "ConfirmDeletes"}),
        "login_start" => json!({"device_name": "pc"}),
        "list_server_folders" => json!({"parent": null, "page": null}),
        "add_pair" => {
            json!({"target": {"NodeId": {"node_id": NODE_A}}, "local_root": "/r", "rules_text": ""})
        }
        "set_settings" => {
            json!({"bandwidth_limit": 1, "hash_throttle": null, "autostart": null, "tray": null})
        }
        _ => json!({}),
    }
}

#[test]
fn dispatch_routes_every_method_of_the_table_to_its_trait_method_with_a_result_of_the_declared_type()
 {
    let daemon = FakeDaemon::new();
    let (_, sink) = sink();

    for info in METHODS {
        let line = serde_json::to_vec(&json!({"jsonrpc": "2.0", "id": 1, "method": info.name, "params": sample_params(info.name)})).unwrap();
        let decoded = decode_call(&line).expect("decodes");

        let response = block_on(dispatch(&daemon, &sink, decoded.id, decoded.call.clone()));

        match response.outcome {
            Outcome::Success { result } => decoded
                .call
                .validate_result(result)
                .unwrap_or_else(|e| panic!("{}: result is not {}: {e}", info.name, info.result)),
            Outcome::Failure { error } => {
                assert_eq!(error.code, CONTROL_ERROR_CODE, "{}", info.name)
            }
        }
    }

    let recorded = daemon.calls.lock().unwrap().clone();
    let expected: Vec<&str> = METHODS.iter().map(|m| m.name).collect();
    assert_eq!(
        recorded, expected,
        "each table entry reaches its own trait method, in order"
    );
}

#[test]
fn dispatch_maps_control_errors_to_32000() {
    let daemon = FakeDaemon::new();
    let (_, sink) = sink();

    let response = call(
        &daemon,
        &sink,
        "update_pair",
        json!({"pair_id": 1, "patch": {"rules_text": "x"}}),
    );

    assert_eq!(response.id, Some(RequestId::Number(1)));
    match response.outcome {
        Outcome::Failure { error } => {
            assert_eq!(error.code, CONTROL_ERROR_CODE);
            assert_eq!(error.data, Some(json!({"kind": "RulesInvalid", "line": 3})));
        }
        Outcome::Success { .. } => panic!("expected a failure"),
    }
}

#[test]
fn dispatch_event_shape_pause_after_subscribe_sends_the_full_status_with_the_pair_paused() {
    let daemon = FakeDaemon::new();
    let (collecting, sink) = sink();

    let subscribed = call(&daemon, &sink, "subscribe", json!({}));
    let paused = call(&daemon, &sink, "pause", json!({"pair_id": 1}));
    let unknown = call(&daemon, &sink, "pause", json!({"pair_id": 99}));

    assert_eq!(subscribed.into_result::<()>().unwrap(), ());
    assert_eq!(paused.into_result::<()>().unwrap(), ());
    assert!(matches!(
        unknown.into_result::<()>(),
        Err(CallError::Control(ControlError::UnknownPair))
    ));
    let events = collecting.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].status.pairs[0].state, PairState::Paused);
    assert_eq!(
        events[0].status.skipped_count, 4,
        "the event carries the complete status"
    );
}
