#![cfg(target_os = "linux")]

mod counting;
mod hold;
mod resolve;
mod root;

use std::fs;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use super::{DeleteGuard, create_sentinel};
use crate::local_index::{Entry, EntryState, LocalIndexStore, NewPair};
use crate::platform_fs::linux::LinuxFs;
use crate::platform_fs::{FileId, PlatformFs};

pub(super) const THRESHOLD: u32 = 200;

/// A pair on a real directory tree with a real index file, as `add_pair` leaves it.
pub(super) struct Fixture {
    pub data: TempDir,
    pub root: TempDir,
    pub fs: LinuxFs,
    pub index: Arc<LocalIndexStore>,
    pub guard: DeleteGuard,
    pub pair_id: i64,
    pub sentinel: String,
}

pub(super) fn fixture() -> Fixture {
    let data = tempfile::tempdir().expect("data dir");
    let root = tempfile::tempdir().expect("root dir");
    let fs = LinuxFs::new();
    let handle = fs.open_root(root.path()).expect("open root");
    let sentinel = create_sentinel(&fs, &handle).expect("create sentinel");
    let index = Arc::new(LocalIndexStore::open(data.path()).expect("open index"));
    let pair_id = index
        .add_pair(&NewPair {
            server_node_id: uuid::Uuid::from_u128(7),
            local_root: root.path().to_str().expect("utf-8 path").to_owned(),
            rules: String::new(),
            root_sentinel: sentinel.clone(),
            case_insensitive: false,
        })
        .expect("add pair");
    let guard = guard_of(&index);
    Fixture {
        data,
        root,
        fs,
        index,
        guard,
        pair_id,
        sentinel,
    }
}

pub(super) fn guard_of(index: &Arc<LocalIndexStore>) -> DeleteGuard {
    DeleteGuard::new(
        Arc::clone(index),
        NonZeroU32::new(THRESHOLD).expect("non-zero"),
    )
}

impl Fixture {
    /// Creates `count` files named `<dir>/f<n>` on disk and in the index, plus the directory entry itself.
    pub fn add_files(&self, dir: &str, count: u64) -> Vec<String> {
        fs::create_dir_all(self.root.path().join(dir)).expect("dir");
        let mut entries = vec![self.entry(dir, 0)];
        let mut paths = Vec::new();
        for n in 1..=count {
            let path = format!("{dir}/f{n}");
            fs::write(self.root.path().join(&path), b"x").expect("file");
            entries.push(self.entry(&path, n));
            paths.push(path);
        }
        self.index.upsert_entries(&entries).expect("index entries");
        paths
    }

    pub fn add_entry_template(&self) -> Entry {
        self.entry("template", 0)
    }

    fn entry(&self, rel_path: &str, n: u64) -> Entry {
        Entry {
            pair_id: self.pair_id,
            rel_path: rel_path.to_owned(),
            node_id: None,
            revision_id: None,
            content_hash: None,
            size: 1,
            mtime_ns: 0,
            file_id: FileId {
                volume: 1,
                index: n + 1,
            },
            state: EntryState::Synced,
            settle_until: None,
            escaped_from: None,
        }
    }

    pub fn state(&self) -> crate::local_index::PairState {
        self.index
            .get_pair(self.pair_id)
            .expect("read pair")
            .expect("pair exists")
            .state
    }

    pub fn sentinel_path(&self) -> std::path::PathBuf {
        self.root.path().join(super::SENTINEL_NAME)
    }

    /// Empties the root of everything but the sentinel, as a user (or a failed mount) would.
    pub fn empty_root(&self) {
        for item in fs::read_dir(self.root.path()).expect("list root") {
            let path = item.expect("dir item").path();
            if path != self.sentinel_path() {
                remove(&path);
            }
        }
    }
}

fn remove(path: &Path) {
    if path.is_dir() {
        fs::remove_dir_all(path).expect("remove dir");
    } else {
        fs::remove_file(path).expect("remove file");
    }
}

pub(super) fn strings(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}
