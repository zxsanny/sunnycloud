//! Where the matrices run: the Mode S server over per-account fakes and recording stubs, or a real stack at a base
//! URL with its own credentials and ids (E-21/c20). The runner sees one [`Target`] either way; only Mode S can
//! look at what the ports received.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use serde_json::Value;
use sunnycloud_http_api::{ApiServices, HttpServer};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::document::{IdParam, Operation};
use super::gates::Gate;
use super::samples;
use super::synth::Built;
use super::world::{GateCall, PER_ACCOUNT_FAKES, World};
use crate::auth::support::{
    ADMIN_TOKEN, MEMBER_ACCOUNT, MEMBER_SESSION, MEMBER_TOKEN, ScriptedAuthenticator,
    ScriptedWorkerAuth,
};
use crate::harness::recording::*;
use crate::harness::recording_identity::*;
use crate::harness::recording_stream::{RecordingFeed, RecordingUploads};
use crate::harness::{
    EVENT_BOUND, SESSION_COOKIE, Script, default_config, new_metrics, stub_services,
};
use crate::sharing::support::{REVISION as SHARED_REVISION, SHARE_ACCOUNT};
use crate::worker::support::Reply;

/// The session tokens of account B (a member) and of an admin, and the cookie name a browser sends them in.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub cookie: String,
    pub member: String,
    pub admin: String,
}

/// A real stack: its listeners, `public_url`, credentials, and for every resource kind A's id and B's own id.
#[derive(Debug, Clone)]
pub struct RemoteConfig {
    pub public: SocketAddr,
    pub internal: Option<SocketAddr>,
    pub host: String,
    pub public_url: String,
    pub credentials: Credentials,
    pub ids_a: HashMap<String, String>,
    pub ids_b: HashMap<String, String>,
}

struct Stub {
    script: Script,
    world: Arc<World>,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

enum Backend {
    Stub(Stub),
    Remote {
        ids_a: HashMap<String, String>,
        ids_b: HashMap<String, String>,
    },
}

/// What the Mode S ports saw since a [`Mark`].
pub struct Seen {
    pub log: Vec<String>,
    pub gate_calls: Vec<GateCall>,
    pub state_changes: usize,
}

#[derive(Clone, Copy)]
pub struct Mark {
    log: usize,
    gates: usize,
    state: usize,
}

pub struct Target {
    pub public: SocketAddr,
    pub internal: Option<SocketAddr>,
    pub host: String,
    pub public_url: String,
    pub credentials: Credentials,
    random: AtomicU64,
    backend: Backend,
}

/// The resource kind an ID parameter names: the collection before a path variable, or the parameter name.
pub fn kind_of(operation: &Operation, param: &IdParam) -> String {
    if param.location == "path" {
        let marker = format!("{{{}}}", param.name);
        let segments: Vec<&str> = operation.path.split('/').collect();
        if let Some(at) = segments.iter().position(|segment| *segment == marker)
            && at > 0
        {
            return segments[at - 1].to_owned();
        }
    }
    param.name.clone()
}

impl Target {
    /// Mode S: the whole server over the per-account fakes; B is the member of the test authenticator.
    pub async fn stub() -> Target {
        let script = Script::default();
        let world = Arc::new(World::default());
        world.own(MEMBER_ACCOUNT, MEMBER_SESSION);
        world.own(SHARE_ACCOUNT, SHARED_REVISION);
        let services = ApiServices {
            authenticator: Arc::new(ScriptedAuthenticator::default()),
            worker_auth: Arc::new(ScriptedWorkerAuth::default()),
            sign_in: RecordingSignIn::new(&script),
            sessions: Gate::new(RecordingSessions::new(&script), &world),
            device_grants: Gate::new(RecordingDeviceGrants::new(&script), &world),
            invites: RecordingInvites::new(&script),
            admin: RecordingAdmin::new(&script),
            accounts: RecordingAccounts::new(&script),
            account_deletion: RecordingDeletion::new(&script),
            tree: Gate::new(RecordingTree::new(&script), &world),
            tree_reader: Gate::new(RecordingTreeReader::new(&script), &world),
            uploads: Gate::new(RecordingUploads::new(&script), &world),
            commits: Gate::new(RecordingCommits::new(&script), &world),
            downloads: Gate::new(RecordingDownloads::new(&script), &world),
            change_feed: RecordingFeed::new(&script),
            shares: Gate::new(RecordingShares::new(&script), &world),
            public_shares: RecordingPublicShares::new(&script),
            derived: Gate::new(RecordingDerived::new(&script), &world),
            thumbnail_states: RecordingThumbnailStates::new(&script),
            ..stub_services(&script, new_metrics())
        };
        let config = default_config();
        let public_url = config.public_url.clone();
        let bound = HttpServer::new(services, config)
            .expect("server builds")
            .bind()
            .await
            .expect("listeners bind");
        let (public, internal) = (bound.public_addr(), bound.internal_addr());
        let (stop, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            bound
                .serve(async move {
                    signal.await.ok();
                })
                .await
                .expect("serve returns");
        });
        Target {
            public,
            internal: Some(internal),
            host: "sunnycloud.test".to_owned(),
            public_url,
            credentials: Credentials {
                cookie: SESSION_COOKIE.to_owned(),
                member: MEMBER_TOKEN.to_owned(),
                admin: ADMIN_TOKEN.to_owned(),
            },
            random: AtomicU64::new(0),
            backend: Backend::Stub(Stub {
                script,
                world,
                stop,
                task,
            }),
        }
    }

    /// A real stack; nothing is started or stopped here.
    pub fn remote(config: RemoteConfig) -> Target {
        Target {
            public: config.public,
            internal: config.internal,
            host: config.host,
            public_url: config.public_url,
            credentials: config.credentials,
            random: AtomicU64::new(0),
            backend: Backend::Remote {
                ids_a: config.ids_a,
                ids_b: config.ids_b,
            },
        }
    }

    pub async fn stop(self) {
        if let Backend::Stub(stub) = self.backend {
            stub.stop.send(()).expect("server is running");
            tokio::time::timeout(EVENT_BOUND, stub.task)
                .await
                .expect("serve returns after the signal")
                .expect("serve does not panic");
        }
    }

    pub fn is_stub(&self) -> bool {
        matches!(self.backend, Backend::Stub(_))
    }

    /// Mode S case data for `owner`; a real stack answers for itself.
    pub fn has_case_data(&self, owner: &str) -> bool {
        !self.is_stub() || samples::has_case_data(owner)
    }

    pub fn has_per_account_fake(&self, owner: &str) -> bool {
        !self.is_stub() || PER_ACCOUNT_FAKES.contains(&owner)
    }

    /// Scripts the success of `owner`'s port calls in Mode S.
    pub fn prepare(&self, owner: &str) {
        if let Backend::Stub(stub) = &self.backend {
            samples::prepare(&stub.script, owner);
        }
    }

    pub fn id_a(&self, operation: &Operation, param: &IdParam) -> Result<String, String> {
        self.id_of(operation, param, true)
    }

    pub fn id_b(&self, operation: &Operation, param: &IdParam) -> Result<String, String> {
        self.id_of(operation, param, false)
    }

    fn id_of(&self, operation: &Operation, param: &IdParam, a: bool) -> Result<String, String> {
        match &self.backend {
            Backend::Stub(stub) => {
                let owner = if a {
                    super::world::ACCOUNT_A
                } else {
                    MEMBER_ACCOUNT
                };
                Ok(stub.world.mint(Some(owner)).to_string())
            }
            Backend::Remote { ids_a, ids_b } => {
                let kind = kind_of(operation, param);
                let ids = if a { ids_a } else { ids_b };
                ids.get(&kind)
                    .cloned()
                    .ok_or(format!("{}: no id of kind {kind} given", operation.label()))
            }
        }
    }

    /// An id no account owns.
    pub fn random_id(&self) -> String {
        match &self.backend {
            Backend::Stub(stub) => stub.world.mint(None).to_string(),
            Backend::Remote { .. } => {
                let n = self.random.fetch_add(1, Ordering::SeqCst);
                Uuid::from_u128(0x0BAD_5EED_0000_0000_0000 + u128::from(n)).to_string()
            }
        }
    }

    pub fn mark(&self) -> Option<Mark> {
        let Backend::Stub(stub) = &self.backend else {
            return None;
        };
        Some(Mark {
            log: stub.script.log().len(),
            gates: stub.world.calls().len(),
            state: stub.script.state_changes(),
        })
    }

    pub fn seen_since(&self, mark: Option<Mark>) -> Option<Seen> {
        let (Backend::Stub(stub), Some(mark)) = (&self.backend, mark) else {
            return None;
        };
        Some(Seen {
            log: stub.script.log().split_off(mark.log),
            gate_calls: stub.world.calls().split_off(mark.gates),
            state_changes: stub.script.state_changes() - mark.state,
        })
    }

    /// `built` with `extra` headers, to the listener its operation is on.
    pub async fn send(&self, built: &Built, extra: &[(String, String)]) -> Reply {
        let addr = if built.internal {
            self.internal.expect("the target has an internal listener")
        } else {
            self.public
        };
        let (content_type, body) = built.body();
        let stream = TcpStream::connect(addr).await.expect("tcp connect");
        let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
            .await
            .expect("h1 handshake");
        tokio::spawn(connection);
        let mut builder = Request::builder()
            .method(built.method.as_str())
            .uri(built.target.as_str())
            .header("host", self.host.as_str());
        if let Some(content_type) = content_type {
            builder = builder
                .header("content-type", content_type)
                .header("content-length", body.len());
        }
        for (name, value) in built.headers.iter().chain(extra) {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let request = builder.body(Full::new(Bytes::from(body))).expect("request");
        let response = tokio::time::timeout(EVENT_BOUND, sender.send_request(request))
            .await
            .expect("the answer comes in time")
            .expect("h1 response");
        let (head, incoming) = response.into_parts();
        let body = incoming.collect().await.expect("body").to_bytes().to_vec();
        Reply {
            status: head.status.as_u16(),
            headers: head.headers,
            body,
        }
    }
}

/// The body of an answer to compare a pair: a problem without its `request_id`, anything else as sent.
pub fn comparable(reply: &Reply) -> (u16, Value) {
    let body = match serde_json::from_slice::<Value>(&reply.body) {
        Ok(Value::Object(mut members)) => {
            members.remove("request_id");
            Value::Object(members)
        }
        Ok(other) => other,
        Err(_) => Value::String(String::from_utf8_lossy(&reply.body).into_owned()),
    };
    (reply.status, body)
}

/// `RemoteConfig` from `SUNNYCLOUD_MATRIX_*`, for E-21/c20; `None` when no base URL is set.
pub fn remote_from_env() -> Option<Result<RemoteConfig, String>> {
    let base = std::env::var("SUNNYCLOUD_MATRIX_BASE_URL").ok()?;
    Some(remote_config(&base, &|name| std::env::var(name).ok()))
}

fn socket_of(url: &str) -> Result<(SocketAddr, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or(format!(
            "{url}: only http:// is supported (no TLS client in the test dependencies)"
        ))?
        .trim_end_matches('/');
    let addr: SocketAddr = rest
        .parse()
        .map_err(|error| format!("{url}: {error} (give ip:port)"))?;
    Ok((addr, rest.to_owned()))
}

pub fn remote_config(
    base: &str,
    var: &dyn Fn(&str) -> Option<String>,
) -> Result<RemoteConfig, String> {
    let need = |name: &str| var(name).ok_or(format!("{name} is not set"));
    let (public, host) = socket_of(base)?;
    let internal = match var("SUNNYCLOUD_MATRIX_INTERNAL_URL") {
        Some(url) => Some(socket_of(&url)?.0),
        None => None,
    };
    let ids: Value = serde_json::from_str(&need("SUNNYCLOUD_MATRIX_IDS")?)
        .map_err(|error| format!("SUNNYCLOUD_MATRIX_IDS: {error}"))?;
    let table = |side: &str| -> HashMap<String, String> {
        ids[side]
            .as_object()
            .map(|kinds| {
                kinds
                    .iter()
                    .filter_map(|(kind, id)| Some((kind.clone(), id.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(RemoteConfig {
        public,
        internal,
        host,
        public_url: need("SUNNYCLOUD_MATRIX_PUBLIC_URL")?,
        credentials: Credentials {
            cookie: need("SUNNYCLOUD_MATRIX_COOKIE")?,
            member: need("SUNNYCLOUD_MATRIX_MEMBER_TOKEN")?,
            admin: need("SUNNYCLOUD_MATRIX_ADMIN_TOKEN")?,
        },
        ids_a: table("a"),
        ids_b: table("b"),
    })
}
