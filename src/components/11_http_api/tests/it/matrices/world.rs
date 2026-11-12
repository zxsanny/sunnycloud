//! The two-account world of Mode S: which account owns which id, and every call the per-account fakes saw.
//! A per-account fake ([`super::gates`]) answers the port's own not-found outcome for an id its caller does not
//! own (another account's or an unknown one) and passes an owned one on to the recording stub.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use uuid::Uuid;

/// Account A owns the ids the matrix aims at; it holds no credential in Mode S.
pub const ACCOUNT_A: Uuid = Uuid::from_u128(0xAAAA);

/// The port methods that have a per-account fake: the IDOR generator gives an ID parameter a case only when its
/// operation's owner is here. `UploadService::stage_small` keys are per account (03 section 4: a row is looked up
/// by `(account, staged_key)`), so its fake answers for the caller's own key space.
pub const PER_ACCOUNT_FAKES: [&str; 21] = [
    "TreeService::create_folder",
    "TreeService::update_node",
    "TreeService::delete_node",
    "TreeReader::get_node",
    "TreeReader::list_children",
    "TreeReader::get_revision",
    "UploadService::start_upload",
    "UploadService::get_upload",
    "UploadService::abort_upload",
    "UploadService::put_block",
    "UploadService::stage_small",
    "CommitService::commit_upload",
    "CommitService::commit_batch",
    "DownloadService::open_revision_content",
    "DerivedOpener::open_derived",
    "ShareService::create",
    "ShareService::list",
    "ShareService::revoke",
    "DeviceGrantService::decide",
    "DeviceGrantService::revoke_device",
    "SessionService::revoke",
];

/// One call a per-account fake received.
#[derive(Debug, Clone)]
pub struct GateCall {
    pub key: &'static str,
    pub account: Uuid,
    pub ids: Vec<Uuid>,
    pub owned: bool,
}

#[derive(Default)]
pub struct World {
    owners: Mutex<HashMap<Uuid, Uuid>>,
    calls: Mutex<Vec<GateCall>>,
    next: AtomicU64,
}

impl World {
    /// A fresh id owned by `owner`, or by no one (a random id).
    pub fn mint(&self, owner: Option<Uuid>) -> Uuid {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        let id = Uuid::from_u128(0x5EED_0000_0000_0000 + u128::from(n));
        if let Some(owner) = owner {
            self.own(owner, id);
        }
        id
    }

    pub fn own(&self, owner: Uuid, id: Uuid) {
        self.owners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, owner);
    }

    /// Records the call and says whether `account` owns every one of `ids`.
    pub fn admit(&self, key: &'static str, account: Uuid, ids: &[Uuid]) -> bool {
        let owned = {
            let owners = self.owners.lock().unwrap_or_else(PoisonError::into_inner);
            ids.iter().all(|id| owners.get(id) == Some(&account))
        };
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(GateCall {
                key,
                account,
                ids: ids.to_vec(),
                owned,
            });
        owned
    }

    /// Whether `account` owns `id`, without recording a call.
    pub fn owns(&self, account: Uuid, id: Uuid) -> bool {
        self.owners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            == Some(&account)
    }

    pub fn calls(&self) -> Vec<GateCall> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}
