//! AC-1 and AC-2 of AZ-3967: the host traits are declared once, in `sync-core`, and the in-memory store is consistent.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use sync_core::{
    BatchRecord, CheckpointStore, MemoryCheckpointStore, UploadCheckpoint, UploadState,
};
use sync_core_proto::{BatchEntry, FileHash, Hash32};
use uuid::Uuid;

const TRAITS: [&str; 5] = [
    "CredentialProvider",
    "FileSource",
    "DownloadSink",
    "CheckpointStore",
    "ProgressSink",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("sync-core sits in the workspace root")
        .to_path_buf()
}

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("directory is readable") {
        let path = entry.expect("directory entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() && name != "target" && !name.starts_with('.') {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// Files that declare `kind name` (`trait X`, `struct X`) at the start of an item.
fn declarations(files: &[PathBuf], kind: &str, name: &str) -> Vec<PathBuf> {
    let needles = [
        format!("{kind} {name} "),
        format!("{kind} {name}{{"),
        format!("{kind} {name}:"),
    ];
    files
        .iter()
        .filter(|file| {
            fs::read_to_string(file)
                .expect("rust file is readable")
                .lines()
                .map(str::trim_start)
                .map(|line| line.strip_prefix("pub ").unwrap_or(line))
                .any(|line| {
                    needles
                        .iter()
                        .any(|needle| line.starts_with(needle.as_str()))
                })
        })
        .cloned()
        .collect()
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    rust_files(dir, &mut found);
    found
}

fn word_in(file: &Path, word: &str) -> bool {
    let text = fs::read_to_string(file).expect("rust file is readable");
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        !is_word(before) && !is_word(after)
    })
}

#[test]
fn the_traits_and_credential_are_declared_once_in_sync_core_and_not_in_contracts() {
    let root = workspace_root();
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    let own_src = root.join("sync-core").join("src");

    let mut declared: Vec<(&str, Vec<PathBuf>)> = TRAITS
        .iter()
        .map(|name| (*name, declarations(&files, "trait", name)))
        .collect();
    // `contracts` has an unrelated server-side `Credential` (identity model), so the client type is
    // checked inside `sync-core` only; the five traits are checked everywhere.
    let credential = declarations(&files_in(&own_src), "struct", "Credential");

    declared.push(("Credential", credential));
    for (name, sites) in &declared {
        assert_eq!(sites.len(), 1, "{name} declared at {sites:?}");
        assert!(
            sites[0].starts_with(&own_src),
            "{name} declared at {:?}",
            sites[0]
        );
    }
    let contracts = root.join("contracts");
    let mut contract_files = Vec::new();
    if contracts.exists() {
        rust_files(&contracts, &mut contract_files);
    }
    for file in &contract_files {
        for name in TRAITS {
            assert!(!word_in(file, name), "{name} named in {file:?}");
        }
    }
    if let Ok(manifest) = fs::read_to_string(contracts.join("Cargo.toml")) {
        assert!(
            !manifest.contains("sync-core"),
            "contracts depends on a client crate"
        );
    }
}

#[test]
fn checkpoint_store_has_no_cursor_method() {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/host/checkpoint.rs"))
            .expect("source is readable");
    let body = source
        .split_once("pub trait CheckpointStore")
        .expect("trait is declared here")
        .1;
    let body = body.split_once("\n}").expect("trait body ends").0;

    let methods: BTreeSet<&str> = body
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("fn "))
        .filter_map(|rest| rest.split('(').next())
        .collect();

    let expected: BTreeSet<&str> = [
        "load",
        "save",
        "clear",
        "save_batch",
        "load_batches",
        "clear_batch",
    ]
    .into_iter()
    .collect();
    assert_eq!(methods, expected);
}

fn checkpoint(seed: u8, state: UploadState) -> UploadCheckpoint {
    UploadCheckpoint {
        session_id: Some(Uuid::from_u128(u128::from(seed))),
        block_hashes: vec![Hash32::from_bytes([seed; 32])],
        file_hash: Some(FileHash::from_bytes([seed; 32])),
        commit_id: Uuid::from_u128(u128::from(seed) << 8),
        state,
    }
}

fn batch(id: u128) -> BatchRecord {
    BatchRecord {
        commit_id: Uuid::from_u128(id),
        entries: vec![BatchEntry {
            staged_key: Uuid::from_u128(id + 1),
            parent: Uuid::from_u128(1),
            name: "entry".to_owned(),
            base_revision: None,
            file_hash: FileHash::from_bytes([7; 32]),
            client_meta: None,
            import_hint: None,
        }],
    }
}

#[test]
fn eight_threads_see_their_own_last_saved_values_and_exactly_the_uncleared_batches() {
    let store = MemoryCheckpointStore::new();
    const PER_THREAD: u128 = 50;

    thread::scope(|scope| {
        for t in 0..8u8 {
            let store = &store;
            scope.spawn(move || {
                let key = format!("file-{t}");
                for i in 0..PER_THREAD {
                    let id = (u128::from(t) << 64) | (i << 8);
                    store.save_batch(batch(id)).expect("save batch");
                    store
                        .save(&key, checkpoint(t, UploadState::Hashing))
                        .expect("save");
                    store
                        .save(
                            &key,
                            checkpoint(t.wrapping_add(100), UploadState::Uploading),
                        )
                        .expect("save");
                    store
                        .save("shared", checkpoint(t, UploadState::Committing))
                        .expect("save");
                    assert_eq!(
                        store.load(&key).expect("load"),
                        Some(checkpoint(t.wrapping_add(100), UploadState::Uploading))
                    );
                    if i % 2 == 0 {
                        store.clear_batch(Uuid::from_u128(id)).expect("clear batch");
                    }
                    if i % 10 == 9 {
                        store.clear(&key).expect("clear");
                        assert_eq!(store.load(&key).expect("load"), None);
                    }
                }
            });
        }
    });

    let kept: BTreeSet<Uuid> = store
        .load_batches()
        .expect("load batches")
        .into_iter()
        .map(|b| b.commit_id)
        .collect();
    let expected: BTreeSet<Uuid> = (0..8u8)
        .flat_map(|t| {
            (0..PER_THREAD)
                .filter(|i| i % 2 == 1)
                .map(move |i| Uuid::from_u128((u128::from(t) << 64) | (i << 8)))
        })
        .collect();
    assert_eq!(kept, expected);
    assert!((0..8u8).any(
        |t| store.load("shared").expect("load") == Some(checkpoint(t, UploadState::Committing))
    ));
    for t in 0..8u8 {
        assert_eq!(store.load(&format!("file-{t}")).expect("load"), None);
    }
}
