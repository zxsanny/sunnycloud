use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use contracts::Fault;

/// What an unscripted port call fails with: never a default success.
#[derive(Debug, thiserror::Error)]
#[error("unscripted port call {0}")]
pub struct UnscriptedCall(pub &'static str);

/// The results the test stubs hand out, keyed `"Port::method"`. A key holds a queue; each call takes the
/// front. A call with nothing queued fails as `Unexpected(UnscriptedCall)`.
#[derive(Clone, Default)]
pub struct Script {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    results: HashMap<&'static str, VecDeque<Box<dyn Any + Send>>>,
    calls: HashMap<&'static str, usize>,
    log: Vec<String>,
}

impl Script {
    /// Queues `result` (the full `Result` the port method returns) for the next call of `key`.
    pub fn push<T: Send + 'static>(&self, key: &'static str, result: T) {
        self.lock()
            .results
            .entry(key)
            .or_default()
            .push_back(Box::new(result));
    }

    /// How many times `key` was called, scripted or not.
    pub fn calls(&self, key: &'static str) -> usize {
        self.lock().calls.get(key).copied().unwrap_or(0)
    }

    /// Appends one line to the ordered log of what the recording stubs were called with.
    pub fn record(&self, line: String) {
        self.lock().log.push(line);
    }

    /// Every recorded call, in the order the calls happened.
    pub fn log(&self) -> Vec<String> {
        self.lock().log.clone()
    }

    /// The state counter of the recording stubs: how many recorded calls went to a port method that changes
    /// state ([`STATE_CHANGING_PORT_METHODS`]).
    pub fn state_changes(&self) -> usize {
        self.lock()
            .log
            .iter()
            .filter_map(|line| line.split_whitespace().next())
            .filter(|key| STATE_CHANGING_PORT_METHODS.contains(key))
            .count()
    }

    pub fn take<T: 'static>(&self, key: &'static str) -> Option<T> {
        let mut inner = self.lock();
        *inner.calls.entry(key).or_default() += 1;
        let boxed = inner.results.get_mut(key)?.pop_front()?;
        Some(
            *boxed
                .downcast::<T>()
                .unwrap_or_else(|_| panic!("the value scripted for {key} has the wrong type")),
        )
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The port methods of the recording stubs that write state, from the owners' port contracts (`contracts`).
/// Reads, and the visitor checks of 06 whose only write is an access audit, are not listed.
pub const STATE_CHANGING_PORT_METHODS: [&str; 31] = [
    "TreeService::create_folder",
    "TreeService::update_node",
    "TreeService::delete_node",
    "UploadService::start_upload",
    "UploadService::put_block",
    "UploadService::abort_upload",
    "UploadService::stage_small",
    "CommitService::commit_upload",
    "CommitService::commit_batch",
    "ShareService::create",
    "ShareService::revoke",
    "PublicShareService::verify_password",
    "SignInService::begin",
    "SignInService::complete",
    "SignInService::begin_link",
    "SignInService::complete_link",
    "SignInService::exchange_mobile_code",
    "SessionService::refresh",
    "SessionService::revoke",
    "SessionService::revoke_all",
    "SessionService::touch",
    "DeviceGrantService::request",
    "DeviceGrantService::poll",
    "DeviceGrantService::decide",
    "DeviceGrantService::revoke_device",
    "DeviceGrantService::revoke_self",
    "InviteService::create_invite",
    "InviteService::revoke",
    "AdminService::update_user",
    "AdminService::set_quota",
    "AccountDeletionService::request",
];

pub fn unscripted(key: &'static str) -> Fault {
    Box::new(UnscriptedCall(key))
}
