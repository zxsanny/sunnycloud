#![allow(dead_code)]
//! Shared pieces of the token-lifecycle tests: an authentication server that keeps state (15-minute access
//! tokens on a test clock, rotating refresh tokens with a 30 s reuse grace) and a host credential spy.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use sc_clock::{Clock, ManualClock, MonotonicInstant};
use sc_config::Secret;
use sc_error::ErrorCode;
use serde_json::{Value, json};
use sync_core::{
    Credential, CredentialProvider, CredentialUnavailable, HttpSyncClient, PersistFailed,
};

use crate::connect_support::{config, valid_info};
use crate::support::{Answer, Fault, Fixture, LoggedRequest, Route, Script};

pub const ACCESS_TTL: Duration = Duration::from_secs(15 * 60);
pub const REUSE_GRACE: Duration = Duration::from_secs(30);

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Rotation {
    previous: String,
    successor: Value,
    at: MonotonicInstant,
}

struct State {
    expires: HashMap<String, MonotonicInstant>,
    refresh: String,
    rotation: Option<Rotation>,
    revoked: bool,
    serial: u32,
    drop_next_refresh_response: bool,
    refresh_presented: Vec<String>,
    carried: Vec<(Instant, String)>,
}

/// The server side of a mobile session. Token values carry the sentinels `ACC-SENTINEL-n` / `REF-SENTINEL-n`.
pub struct FakeAuth {
    clock: Arc<ManualClock>,
    stale_delay: Duration,
    state: Mutex<State>,
}

impl FakeAuth {
    /// `stale_delay` holds back every 401 so requests sent together all fail before the first rotation ends.
    pub fn new(clock: Arc<ManualClock>, stale_delay: Duration) -> Arc<Self> {
        let auth = Self {
            clock: Arc::clone(&clock),
            stale_delay,
            state: Mutex::new(State {
                expires: HashMap::new(),
                refresh: String::new(),
                rotation: None,
                revoked: false,
                serial: 0,
                drop_next_refresh_response: false,
                refresh_presented: Vec::new(),
                carried: Vec::new(),
            }),
        };
        {
            let mut state = locked(&auth.state);
            auth.issue(&mut state);
        }
        Arc::new(auth)
    }

    fn issue(&self, state: &mut State) -> Value {
        let n = state.serial;
        state.serial += 1;
        let (access, refresh) = (format!("ACC-SENTINEL-{n}"), format!("REF-SENTINEL-{n}"));
        let expires = self
            .clock
            .monotonic()
            .checked_add(ACCESS_TTL)
            .expect("test clock stays in range");
        state.expires.insert(access.clone(), expires);
        state.refresh = refresh.clone();
        json!({ "access_token": access, "refresh_token": refresh, "expires_in": ACCESS_TTL.as_secs() })
    }

    /// A new sign-in (code exchange or device grant): a fresh valid pair, and the revocation is over.
    pub fn mint_tokens(&self) -> Value {
        let mut state = locked(&self.state);
        state.revoked = false;
        self.issue(&mut state)
    }

    pub fn initial_access() -> String {
        "ACC-SENTINEL-0".to_owned()
    }

    pub fn initial_refresh() -> String {
        "REF-SENTINEL-0".to_owned()
    }

    /// The access token passed 15 minutes: moves the test clock.
    pub fn expire_access(&self) {
        self.clock.tick(ACCESS_TTL);
    }

    /// The session is revoked: every refresh answers 401.
    pub fn revoke(&self) {
        locked(&self.state).revoked = true;
    }

    /// The next refresh is processed (the token rotates) but its response never arrives.
    pub fn lose_next_refresh_response(&self) {
        locked(&self.state).drop_next_refresh_response = true;
    }

    /// Refresh tokens presented to the refresh route, in arrival order.
    pub fn refresh_presented(&self) -> Vec<String> {
        locked(&self.state).refresh_presented.clone()
    }

    /// Access tokens the protected routes saw, with their arrival time.
    pub fn carried(&self) -> Vec<(Instant, String)> {
        locked(&self.state).carried.clone()
    }

    pub fn protected(self: &Arc<Self>, ok: Answer) -> impl Fn(&LoggedRequest) -> Answer + use<> {
        let auth = Arc::clone(self);
        move |request| auth.guard(request, &ok)
    }

    /// `script` plus the refresh route and `routes` as protected routes answering `ok`.
    pub fn wire(self: &Arc<Self>, mut script: Script, routes: &[(Route, Answer)]) -> Script {
        for (route, ok) in routes {
            script = script.respond(*route, self.protected(ok.clone()));
        }
        let auth = Arc::clone(self);
        script.respond(Route::RefreshTokens, move |request| auth.rotate(request))
    }

    fn guard(&self, request: &LoggedRequest, ok: &Answer) -> Answer {
        let token = request
            .header("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or_default()
            .to_owned();
        let mut state = locked(&self.state);
        state.carried.push((Instant::now(), token.clone()));
        let valid = state
            .expires
            .get(&token)
            .is_some_and(|expires| self.clock.monotonic() < *expires);
        if valid {
            ok.clone()
        } else {
            Answer::problem(ErrorCode::Unauthenticated).delay(self.stale_delay)
        }
    }

    fn rotate(&self, request: &LoggedRequest) -> Answer {
        let presented = serde_json::from_slice::<Value>(&request.body)
            .ok()
            .and_then(|body| body["refresh_token"].as_str().map(str::to_owned))
            .unwrap_or_default();
        let mut state = locked(&self.state);
        state.refresh_presented.push(presented.clone());
        if state.revoked {
            return Answer::problem(ErrorCode::Unauthenticated);
        }
        let now = self.clock.monotonic();
        let tokens = if presented == state.refresh {
            let tokens = self.issue(&mut state);
            state.rotation = Some(Rotation {
                previous: presented,
                successor: tokens.clone(),
                at: now,
            });
            tokens
        } else {
            match &state.rotation {
                Some(rotation)
                    if rotation.previous == presented
                        && now < rotation.at.checked_add(REUSE_GRACE).expect("in range") =>
                {
                    rotation.successor.clone()
                }
                _ => {
                    state.revoked = true;
                    return Answer::problem(ErrorCode::Unauthenticated);
                }
            }
        };
        let answer = Answer::json(200, &tokens);
        if std::mem::take(&mut state.drop_next_refresh_response) {
            answer.fault(Fault::DropConnection)
        } else {
            answer
        }
    }
}

/// The host: keeps the pair in memory, takes `persist_delay` to store a rotated one and can be told to fail.
pub struct SpyCredentials {
    pair: Mutex<(String, Option<String>)>,
    persist_delay: Duration,
    failures_left: AtomicU32,
    persisted: Mutex<Vec<(String, String)>>,
    persist_started: Mutex<Vec<Instant>>,
    persist_returned: Mutex<Vec<Instant>>,
    unavailable: AtomicBool,
}

impl SpyCredentials {
    pub fn session(persist_delay: Duration) -> Arc<Self> {
        Self::with(
            FakeAuth::initial_access(),
            Some(FakeAuth::initial_refresh()),
            persist_delay,
        )
    }

    /// A desktop device token: no refresh token.
    pub fn device() -> Arc<Self> {
        Self::with(FakeAuth::initial_access(), None, Duration::ZERO)
    }

    fn with(access: String, refresh: Option<String>, persist_delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            pair: Mutex::new((access, refresh)),
            persist_delay,
            failures_left: AtomicU32::new(0),
            persisted: Mutex::new(Vec::new()),
            persist_started: Mutex::new(Vec::new()),
            persist_returned: Mutex::new(Vec::new()),
            unavailable: AtomicBool::new(false),
        })
    }

    /// While set, `bearer` answers `CredentialUnavailable`, as a locked keyring does.
    pub fn set_unavailable(&self, unavailable: bool) {
        self.unavailable.store(unavailable, Ordering::SeqCst);
    }

    /// The next `n` calls of `on_rotated` fail.
    pub fn fail_persists(&self, n: u32) {
        self.failures_left.store(n, Ordering::SeqCst);
    }

    /// The host stores a pair it got from a sign-in call.
    pub fn store(&self, access: &str, refresh: &str) {
        *locked(&self.pair) = (access.to_owned(), Some(refresh.to_owned()));
    }

    pub fn persisted(&self) -> Vec<(String, String)> {
        locked(&self.persisted).clone()
    }

    pub fn persist_started(&self) -> Vec<Instant> {
        locked(&self.persist_started).clone()
    }

    pub fn persist_returned(&self) -> Vec<Instant> {
        locked(&self.persist_returned).clone()
    }
}

impl CredentialProvider for SpyCredentials {
    fn bearer(&self) -> Result<Credential, CredentialUnavailable> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(CredentialUnavailable);
        }
        let (access, refresh) = locked(&self.pair).clone();
        Ok(Credential {
            access: Secret::new(access),
            refresh: refresh.map(Secret::new),
        })
    }

    /// Keeps the new pair in memory at once and then spends `persist_delay` on the slow store, as a host does.
    fn on_rotated(
        &self,
        access: Secret<String>,
        refresh: Secret<String>,
    ) -> Result<(), PersistFailed> {
        locked(&self.persist_started).push(Instant::now());
        locked(&self.persisted).push((access.expose().clone(), refresh.expose().clone()));
        let failing = self
            .failures_left
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok();
        if !failing {
            *locked(&self.pair) = (access.expose().clone(), Some(refresh.expose().clone()));
        }
        std::thread::sleep(self.persist_delay);
        locked(&self.persist_returned).push(Instant::now());
        if failing { Err(PersistFailed) } else { Ok(()) }
    }
}

/// A client of the real `connect` against a fixture that serves `script`, with the host `credentials`.
pub async fn connected_as(
    script: Script,
    credentials: Arc<dyn CredentialProvider>,
) -> (Arc<HttpSyncClient>, Fixture) {
    let script = script.always(Route::ServerInfo, Answer::json(200, &valid_info()));
    let fixture = Fixture::start(script).await;
    let (api, _) = HttpSyncClient::connect_with_roots(
        config(&fixture.url()),
        credentials,
        vec![fixture.ca().clone()],
    )
    .await
    .expect("the fixture serves server-info");
    (Arc::new(api), fixture)
}

pub fn requests_to(fixture: &Fixture, route: Route) -> Vec<LoggedRequest> {
    fixture
        .requests()
        .into_iter()
        .filter(|request| request.route == Some(route))
        .collect()
}

pub fn root_view() -> Answer {
    Answer::json(
        200,
        &json!({
            "node_id": uuid::Uuid::from_u128(1),
            "parent_id": null,
            "kind": "folder",
            "name": "",
            "current_revision": null,
        }),
    )
}

/// The server's 400 `invalid_request` for one request field (11's `ApiProblem::validation`). For `field` `code`
/// it is the refusal of an unknown, used or expired mobile code.
pub fn field_refused(field: &str, problem: &str) -> Answer {
    Answer::problem(ErrorCode::InvalidRequest)
        .extra("problems", json!([{ "field": field, "problem": problem }]))
}
