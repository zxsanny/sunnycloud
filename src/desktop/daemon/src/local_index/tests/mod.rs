mod cases;
mod checkpoint;
mod crash;
mod damaged;
mod scale;
mod schema;

use sync_core_proto::FileHash;
use tempfile::TempDir;
use uuid::Uuid;

use super::{Entry, EntryState, LocalIndexStore, NewPair};
use crate::platform_fs::FileId;

pub(super) struct Fixture {
    pub dir: TempDir,
    pub store: LocalIndexStore,
}

pub(super) fn open() -> Fixture {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = LocalIndexStore::open(dir.path()).expect("open index");
    Fixture { dir, store }
}

pub(super) fn new_pair(case_insensitive: bool) -> NewPair {
    NewPair {
        server_node_id: Uuid::from_u128(0xA11CE),
        local_root: "/home/user/Documents".to_owned(),
        rules: "{}".to_owned(),
        root_sentinel: "sentinel".to_owned(),
        case_insensitive,
    }
}

pub(super) fn entry(pair_id: i64, rel_path: &str, n: u64) -> Entry {
    Entry {
        pair_id,
        rel_path: rel_path.to_owned(),
        node_id: Some(Uuid::from_u128(u128::from(n) + 1)),
        revision_id: Some(Uuid::from_u128(u128::from(n) + 1_000_000)),
        content_hash: Some(FileHash::from_bytes([n.to_le_bytes()[0]; 32])),
        size: n * 10,
        mtime_ns: i64::try_from(n).expect("small") * 1_000,
        file_id: FileId {
            volume: 1,
            index: n,
        },
        state: EntryState::Synced,
        settle_until: None,
        escaped_from: None,
    }
}
