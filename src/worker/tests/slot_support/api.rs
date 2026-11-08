//! The worker listener as a script: queued jobs per lane, their inputs, and the status each route answers
//! with. Built on the in-process peer of `api_support`; the supervisor's real client talks to it over HTTP.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use media_shared::types::{ClaimedJob, JobKind};
use serde_json::Value;
use uuid::Uuid;

use crate::api_support::{Peer, Recorded, Reply, file_hash};

/// One queued job and how the listener treats it.
pub struct Entry {
    pub job: ClaimedJob,
    pub input: Vec<u8>,
    /// Status of `GET input`; anything but 200 is answered with no body.
    pub input_status: u16,
    /// Status of `PUT outputs/{kind}` per kind (`thumb`, `preview`, `transcode`); 204 when absent.
    pub output_status: BTreeMap<&'static str, u16>,
    pub failure_status: u16,
    pub result_status: u16,
    /// Status of `PUT lease`; 409 once the test expires the lease.
    pub lease_status: u16,
}

impl Entry {
    pub fn new(n: u128, kind: JobKind) -> Self {
        let input = format!("input of job {n}").into_bytes();
        let mut job = crate::api_support::job(kind, file_hash(&input));
        job.job_id = Uuid::from_u128(n);
        Self {
            job,
            input,
            input_status: 200,
            output_status: BTreeMap::new(),
            failure_status: 200,
            result_status: 204,
            lease_status: 200,
        }
    }

    pub fn input_status(mut self, status: u16) -> Self {
        self.input_status = status;
        self
    }

    pub fn output_status(mut self, kind: &'static str, status: u16) -> Self {
        self.output_status.insert(kind, status);
        self
    }

    pub fn failure_status(mut self, status: u16) -> Self {
        self.failure_status = status;
        self
    }

    /// The `lease_ms` the claim carries, and so the heartbeat period (a third of it).
    pub fn lease_ms(mut self, lease_ms: u64) -> Self {
        self.job.lease_ms = lease_ms;
        self
    }

    pub fn lease_status(mut self, status: u16) -> Self {
        self.lease_status = status;
        self
    }

    pub fn params_version(mut self, version: u32) -> Self {
        self.job.params_version = version;
        self
    }

    /// The claimed `content_hash` no longer matches the bytes the listener serves.
    pub fn with_wrong_hash(mut self) -> Self {
        self.job.content_hash = file_hash(b"some other bytes");
        self
    }

    pub fn id(&self) -> Uuid {
        self.job.job_id
    }
}

#[derive(Default)]
struct State {
    image: VecDeque<Entry>,
    video: VecDeque<Entry>,
    taken: BTreeMap<Uuid, Entry>,
    claim_status: Option<u16>,
}

pub struct ScriptedApi {
    pub peer: Peer,
    state: Arc<Mutex<State>>,
}

impl ScriptedApi {
    pub async fn start() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let shared = Arc::clone(&state);
        let peer = Peer::start(move |request| answer(&shared, request)).await;
        Self { peer, state }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues the job on the lane its kind belongs to.
    pub fn enqueue(&self, entry: Entry) {
        let mut state = self.state();
        match entry.job.kind {
            JobKind::Stills => state.image.push_back(entry),
            JobKind::Poster | JobKind::Transcode => state.video.push_back(entry),
        }
    }

    /// Every claim is answered with this status.
    pub fn refuse_claims(&self, status: u16) {
        self.state().claim_status = Some(status);
    }

    /// From now on the heartbeat of `job` is answered 409, as when the lease ran out and the job was handed on.
    pub fn expire_lease(&self, job: Uuid) {
        let mut state = self.state();
        let entry = state
            .taken
            .get_mut(&job)
            .expect("the job was claimed before its lease is expired");
        entry.lease_status = 409;
    }

    pub fn base_url(&self) -> String {
        self.peer.base_url()
    }
}

fn answer(state: &Mutex<State>, request: &Recorded) -> Reply {
    let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
    let route: Vec<&str> = request
        .path
        .trim_start_matches("/api/v1/worker-jobs")
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    match (request.method.as_str(), route.as_slice()) {
        ("POST", ["claims"]) => claim(&mut state, request),
        ("GET", [id, "input"]) => match entry(&state, id) {
            Some(entry) if entry.input_status == 200 => Reply::bytes(entry.input.clone()),
            Some(entry) => Reply::status(entry.input_status),
            None => Reply::status(409),
        },
        ("PUT", [id, "lease"]) => match entry(&state, id) {
            Some(entry) if entry.lease_status == 200 => {
                Reply::json(200, &format!(r#"{{"lease_ms":{}}}"#, entry.job.lease_ms))
            }
            Some(entry) => Reply::status(entry.lease_status),
            None => Reply::status(409),
        },
        ("PUT", [id, "outputs", kind]) => match entry(&state, id) {
            Some(entry) => Reply::status(entry.output_status.get(kind).copied().unwrap_or(204)),
            None => Reply::status(409),
        },
        ("PUT", [id, "result"]) => match entry(&state, id) {
            Some(entry) => Reply::status(entry.result_status),
            None => Reply::status(409),
        },
        ("POST", [id, "failure"]) => match entry(&state, id) {
            Some(entry) if entry.failure_status == 200 => {
                Reply::json(200, r#"{"result":"requeued"}"#)
            }
            Some(entry) => Reply::status(entry.failure_status),
            None => Reply::status(409),
        },
        _ => Reply::status(404),
    }
}

fn entry<'a>(state: &'a State, id: &str) -> Option<&'a Entry> {
    id.parse().ok().and_then(|id| state.taken.get(&id))
}

fn claim(state: &mut State, request: &Recorded) -> Reply {
    if let Some(status) = state.claim_status {
        return Reply::status(status);
    }
    let body: Value = serde_json::from_slice(&request.body).expect("a claim body");
    let queue = match body["lane"].as_str() {
        Some("image") => &mut state.image,
        Some("video") => &mut state.video,
        other => panic!("unknown lane {other:?}"),
    };
    let Some(entry) = queue.pop_front() else {
        return Reply::status(204);
    };
    let body = serde_json::to_string(&entry.job).expect("job json");
    state.taken.insert(entry.job.job_id, entry);
    Reply::json(200, &body)
}
