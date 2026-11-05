//! A contract-conformant `DaemonControl` double (the real one is E-24/c19) and a JSON-RPC
//! line client for the tests.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ipc_schema::*;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;

/// The device token the fake is "signed in" with. It lives inside the fake and no type of
/// the schema can carry it; the tests assert it never shows up on the wire.
pub const TOKEN: &str = "sc-device-token-7f3a9c1e5b2d4086";

const NODE: &str = "0190f3b0-7c2e-7a41-9d5e-2b6c8a1f4e30";

pub struct FakeDaemon {
    state: Mutex<State>,
}

struct State {
    status: DaemonStatus,
    sinks: Vec<Arc<dyn StatusSink>>,
}

fn node() -> NodeId {
    NodeId::parse(NODE).unwrap()
}

impl FakeDaemon {
    pub fn new() -> Arc<Self> {
        let status = DaemonStatus {
            server_url: Some("https://cloud.example".to_owned()),
            account: Some(AccountView {
                account_id: "acct-1".to_owned(),
            }),
            pairs: vec![PairView {
                pair_id: PairId(1),
                local_root: "/home/u/Sync".to_owned(),
                server_node_id: node(),
                state: PairState::Active,
                rules_text: "*.tmp\n".to_owned(),
            }],
            skipped_count: 2,
            escaped_count: 1,
            holds: Vec::new(),
            update_available: None,
            access_denied: Vec::new(),
            watcher_degraded: Vec::new(),
        };
        Arc::new(Self {
            state: Mutex::new(State {
                status,
                sinks: Vec::new(),
            }),
        })
    }

    pub fn subscriber_count(&self) -> usize {
        self.state.lock().unwrap().sinks.len()
    }

    /// Sends the current status to every subscriber, as the daemon does on any change.
    pub fn emit(&self) {
        let state = self.state.lock().unwrap();
        for sink in &state.sinks {
            sink.send(StatusEvent {
                status: state.status.clone(),
            });
        }
    }

    fn set_pair_state(&self, params: &PairIdParams, pair_state: PairState) -> ControlResult<()> {
        {
            let mut state = self.state.lock().unwrap();
            let pair = state
                .status
                .pairs
                .iter_mut()
                .find(|p| p.pair_id == params.pair_id)
                .ok_or(ControlError::UnknownPair)?;
            pair.state = pair_state;
        }
        self.emit();
        Ok(())
    }
}

impl DaemonControl for FakeDaemon {
    async fn status(&self) -> ControlResult<DaemonStatus> {
        Ok(self.state.lock().unwrap().status.clone())
    }

    async fn set_server(&self, params: SetServerParams) -> ControlResult<()> {
        if params.url.starts_with("https://") {
            Ok(())
        } else {
            Err(ControlError::HttpsRequired)
        }
    }

    async fn list_server_folders(&self, _: ListServerFoldersParams) -> ControlResult<FolderPage> {
        Ok(FolderPage {
            items: vec![FolderEntry {
                node_id: node(),
                name: "Docs".to_owned(),
            }],
            next: None,
        })
    }

    async fn list_pairs(&self) -> ControlResult<Vec<PairView>> {
        Ok(self.state.lock().unwrap().status.pairs.clone())
    }

    async fn add_pair(&self, _: PairSpec) -> ControlResult<PairId> {
        Ok(PairId(2))
    }

    async fn update_pair(&self, _: UpdatePairParams) -> ControlResult<()> {
        Ok(())
    }

    async fn remove_pair(&self, _: PairIdParams) -> ControlResult<()> {
        Ok(())
    }

    async fn pause(&self, params: PairIdParams) -> ControlResult<()> {
        self.set_pair_state(&params, PairState::Paused)
    }

    async fn resume(&self, params: PairIdParams) -> ControlResult<()> {
        self.set_pair_state(&params, PairState::Active)
    }

    async fn list_skipped(&self, _: ListEntriesParams) -> ControlResult<SkippedPage> {
        Ok(SkippedPage {
            items: Vec::new(),
            next: None,
        })
    }

    async fn list_escaped(&self, _: ListEntriesParams) -> ControlResult<EscapedPage> {
        Ok(EscapedPage {
            items: Vec::new(),
            next: None,
        })
    }

    async fn get_settings(&self) -> ControlResult<Settings> {
        Ok(Settings {
            bandwidth_limit: 0,
            hash_throttle: 0.5,
            autostart: true,
            tray: true,
        })
    }

    async fn set_settings(&self, patch: SettingsPatch) -> ControlResult<Settings> {
        Ok(Settings {
            bandwidth_limit: patch.bandwidth_limit.unwrap_or(0),
            hash_throttle: patch.hash_throttle.unwrap_or(0.5),
            autostart: patch.autostart.unwrap_or(true),
            tray: patch.tray.unwrap_or(true),
        })
    }

    async fn resolve_hold(&self, _: ResolveHoldParams) -> ControlResult<()> {
        Err(ControlError::NoSuchHold)
    }

    async fn login_start(&self, _: LoginStartParams) -> ControlResult<LoginPrompt> {
        Ok(LoginPrompt {
            verification_url: "https://cloud.example/device".to_owned(),
            user_code: "ABCD-EFGH".to_owned(),
            expires_at: "2026-11-05T12:00:00Z".to_owned(),
        })
    }

    async fn login_status(&self) -> ControlResult<LoginState> {
        // Signed in with TOKEN; the state never carries it.
        Ok(LoginState::Approved)
    }

    async fn logout(&self) -> ControlResult<LogoutOutcome> {
        Ok(LogoutOutcome {
            server_revoked: true,
        })
    }

    async fn subscribe(&self, sink: Arc<dyn StatusSink>) -> ControlResult<()> {
        self.state.lock().unwrap().sinks.push(sink);
        Ok(())
    }
}

const STEP: Duration = Duration::from_secs(10);

/// A JSON-RPC client over any byte stream. Keeps the raw bytes it received.
pub struct Client<S> {
    stream: S,
    lines: LineBuffer,
    next_id: i64,
    pub events: Vec<StatusEvent>,
    pub received: Vec<u8>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Client<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            lines: LineBuffer::default(),
            next_id: 1,
            events: Vec::new(),
            received: Vec::new(),
        }
    }

    pub async fn send_raw(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        timeout(STEP, self.stream.write_all(bytes))
            .await
            .expect("the server stopped reading")
    }

    /// The next message from the server; `None` once the server closed the connection.
    pub async fn next_message(&mut self) -> Option<ServerMessage> {
        loop {
            if let Some(line) = self.lines.next_line() {
                let message = serde_json::from_slice(&line).expect("server sent a bad line");
                if let ServerMessage::Notification(n) = &message {
                    let event = n.as_status_event().unwrap().expect("not a status event");
                    self.events.push(event);
                }
                return Some(message);
            }
            let mut chunk = [0_u8; 8192];
            let read = timeout(STEP, self.stream.read(&mut chunk))
                .await
                .expect("the server sent nothing")
                .unwrap_or(0);
            if read == 0 {
                return None;
            }
            self.received.extend_from_slice(&chunk[..read]);
            self.lines.push(&chunk[..read]);
        }
    }

    /// The next response, taking any notification that arrives first into `events`.
    pub async fn next_response(&mut self) -> Option<Response> {
        loop {
            match self.next_message().await? {
                ServerMessage::Response(response) => return Some(response),
                ServerMessage::Notification(_) => {}
            }
        }
    }

    pub async fn call(&mut self, call: MethodCall) -> Response {
        let id = RequestId::Number(self.next_id);
        self.next_id += 1;
        let line = encode_line(&Request::new(id.clone(), &call).unwrap()).unwrap();
        self.send_raw(&line).await.unwrap();
        let response = self.next_response().await.expect("connection closed");
        assert_eq!(response.id, Some(id));
        response
    }

    /// Reads until `count` status events have arrived in total.
    pub async fn wait_for_events(&mut self, count: usize) {
        while self.events.len() < count {
            self.next_message().await.expect("connection closed");
        }
    }
}

pub fn error_code(response: &Response) -> i64 {
    match &response.outcome {
        Outcome::Failure { error } => error.code,
        Outcome::Success { .. } => panic!("expected an error, got {response:?}"),
    }
}
