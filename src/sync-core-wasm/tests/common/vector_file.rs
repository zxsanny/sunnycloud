//! The shared vector file (`vectors/sync_core_vectors.txt`): parser, the stated byte generator, and the
//! native computation of every case. Used by the native runner (`tests/vectors.rs`) and the generator
//! (`examples/gen_vectors.rs`); the JavaScript runner (`js/run-vectors.mjs`) mirrors the parser.
// Each includer uses a different part of this module.
#![allow(dead_code)]

use std::time::Duration;

use sync_core_proto::{
    Action, BLOCK_SIZE, BlockResult, CommitResult, FileHash, Hash32, RetryPolicy, UploadPlanner,
};
use uuid::{Builder, Uuid};

/// The committed vector file, relative to the crate.
pub const DEFAULT_PATH: &str = "vectors/sync_core_vectors.txt";

#[derive(Clone, Debug, PartialEq)]
pub enum Case {
    File {
        name: String,
        length: u64,
        file_hash: String,
        blocks: Vec<String>,
    },
    Oversize {
        name: String,
        length: u64,
    },
    ClientKey {
        name: String,
        device: String,
        asset: String,
        file_hash: String,
        key: String,
    },
    CommitId {
        name: String,
        unix_ms: u64,
        random: String,
        id: String,
    },
    Planner(PlannerCase),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlannerCase {
    pub name: String,
    pub block_count: u32,
    pub need: Vec<u32>,
    pub lanes_max: u32,
    pub retry_base_ms: u32,
    pub retry_max_ms: u32,
    pub max_attempts: u32,
    pub events: Vec<String>,
    /// Action lists, one per `next_actions` call: the first call, then one after each event.
    pub expected: String,
}

/// Byte `i` of every generated file is `i mod 251` (the `sc_ids` generator). Fills `buf` with the bytes
/// at file offset `offset`.
pub fn fill_test_bytes(buf: &mut [u8], offset: u64) {
    let mut value = (offset % 251) as u8;
    for byte in buf {
        *byte = value;
        value = if value == 250 { 0 } else { value + 1 };
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err(format!("odd hex length: {text}"));
    }
    (0..text.len())
        .step_by(2)
        .map(|at| {
            u8::from_str_radix(&text[at..at + 2], 16).map_err(|e| format!("bad hex {text}: {e}"))
        })
        .collect()
}

fn list(text: &str) -> Result<Vec<u32>, String> {
    if text == "-" {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|n| n.parse().map_err(|e| format!("bad index {n}: {e}")))
        .collect()
}

fn num<T: std::str::FromStr>(text: &str) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    text.parse().map_err(|e| format!("bad number {text}: {e}"))
}

/// Parses every non-comment line; a malformed line is an error naming it.
pub fn parse(text: &str) -> Result<Vec<Case>, String> {
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| parse_line(line).map_err(|e| format!("malformed line `{line}`: {e}")))
        .collect()
}

fn parse_line(line: &str) -> Result<Case, String> {
    let f: Vec<&str> = line.split(' ').collect();
    let s = |i: usize| f[i].to_string();
    match (f[0], f.len()) {
        ("file", 5) => Ok(Case::File {
            name: s(1),
            length: num(f[2])?,
            file_hash: s(3),
            blocks: match f[4] {
                "-" => Vec::new(),
                blocks => blocks.split(',').map(str::to_string).collect(),
            },
        }),
        ("oversize", 3) => Ok(Case::Oversize {
            name: s(1),
            length: num(f[2])?,
        }),
        ("client_key", 6) => Ok(Case::ClientKey {
            name: s(1),
            device: s(2),
            asset: s(3),
            file_hash: s(4),
            key: s(5),
        }),
        ("commit_id", 5) => Ok(Case::CommitId {
            name: s(1),
            unix_ms: num(f[2])?,
            random: s(3),
            id: s(4),
        }),
        ("planner", 10) => Ok(Case::Planner(PlannerCase {
            name: s(1),
            block_count: num(f[2])?,
            need: list(f[3])?,
            lanes_max: num(f[4])?,
            retry_base_ms: num(f[5])?,
            retry_max_ms: num(f[6])?,
            max_attempts: num(f[7])?,
            events: match f[8] {
                "-" => Vec::new(),
                events => events.split(';').map(str::to_string).collect(),
            },
            expected: s(9),
        })),
        (kind, n) => Err(format!("unknown kind `{kind}` with {n} fields")),
    }
}

/// Native block hashes and file hash of a generated file of `length` bytes, hashed block by block.
pub fn native_file(length: u64) -> (Vec<String>, String) {
    let mut buf = vec![0u8; BLOCK_SIZE];
    let mut hashes = Vec::new();
    let mut offset = 0u64;
    while offset < length {
        let len = usize::try_from((length - offset).min(BLOCK_SIZE as u64))
            .expect("a block length fits usize");
        let block = &mut buf[..len];
        fill_test_bytes(block, offset);
        hashes.push(sync_core_proto::hash_block(block).expect("a generated block fits"));
        offset += len as u64;
    }
    let file = sync_core_proto::rollup(&hashes);
    (
        hashes.iter().map(|h| hex(h.as_bytes())).collect(),
        hex(file.as_bytes()),
    )
}

pub fn native_client_key(device: &str, asset: &str, file_hash: &str) -> Result<String, String> {
    let device = Uuid::parse_str(device).map_err(|e| format!("device id: {e}"))?;
    let hash: [u8; 32] = unhex(file_hash)?
        .try_into()
        .map_err(|bytes: Vec<u8>| format!("file hash of {} bytes", bytes.len()))?;
    let key = sync_core_proto::client_key_for_mobile(device, asset, &FileHash::from_bytes(hash));
    Ok(key.to_string())
}

/// UUIDv7 straight from the `uuid` builder, independent of the adapter's own function.
pub fn native_commit_id(unix_ms: u64, random: &str) -> Result<String, String> {
    let random: [u8; 10] = unhex(random)?
        .try_into()
        .map_err(|bytes: Vec<u8>| format!("random of {} bytes", bytes.len()))?;
    Ok(Builder::from_unix_timestamp_millis(unix_ms, &random)
        .into_uuid()
        .to_string())
}

fn encode_action(action: Action) -> String {
    match action {
        Action::SendBlock { i } => format!("SendBlock:{i}"),
        Action::Wait { ms } => format!("Wait:{ms}"),
        Action::RereadSession => "RereadSession".to_string(),
        Action::Commit => "Commit".to_string(),
        Action::Done => "Done".to_string(),
        Action::RestartSession => "RestartSession".to_string(),
        Action::GiveUp { i } => format!("GiveUp:{i}"),
    }
}

fn encode_list(actions: Vec<Action>) -> String {
    if actions.is_empty() {
        return "-".to_string();
    }
    actions
        .into_iter()
        .map(encode_action)
        .collect::<Vec<_>>()
        .join(",")
}

fn retry(text: &str) -> Result<Option<Duration>, String> {
    match text {
        "-" => Ok(None),
        ms => Ok(Some(Duration::from_millis(num(ms)?))),
    }
}

fn apply(planner: &mut UploadPlanner, event: &str) -> Result<(), String> {
    let f: Vec<&str> = event.split(':').collect();
    match f[..] {
        ["Acked", i] => planner.on_result(num(i)?, BlockResult::Acked),
        ["HashRejected", i] => planner.on_result(num(i)?, BlockResult::HashRejected),
        ["Transient", i, ms] => planner.on_result(
            num(i)?,
            BlockResult::Transient {
                retry_after: retry(ms)?,
            },
        ),
        ["BlockConflict", i, ms] => planner.on_result(
            num(i)?,
            BlockResult::BlockConflict {
                retry_after: retry(ms)?,
            },
        ),
        ["SessionGone", i] => planner.on_result(num(i)?, BlockResult::SessionGone),
        ["Session", need] => planner.on_session(&list(need)?),
        ["Commit", "Ok"] => planner.on_commit(CommitResult::Ok),
        ["Commit", "BlocklistMismatch"] => planner.on_commit(CommitResult::BlocklistMismatch),
        ["Commit", "SessionGone"] => planner.on_commit(CommitResult::SessionGone),
        ["Commit", "NeedBlocks", need] => {
            planner.on_commit(CommitResult::NeedBlocks { list: list(need)? })
        }
        _ => return Err(format!("unknown planner event `{event}`")),
    }
    Ok(())
}

/// Native action lists of a planner case, encoded as in the vector file.
pub fn native_planner(case: &PlannerCase) -> Result<String, String> {
    let policy = RetryPolicy {
        base: Duration::from_millis(u64::from(case.retry_base_ms)),
        max: Duration::from_millis(u64::from(case.retry_max_ms)),
        max_attempts_per_block: case.max_attempts,
    };
    let mut planner = UploadPlanner::new(case.block_count, &case.need, case.lanes_max, policy);
    let mut lists = vec![encode_list(planner.next_actions())];
    for event in &case.events {
        apply(&mut planner, event)?;
        lists.push(encode_list(planner.next_actions()));
    }
    Ok(lists.join("/"))
}

/// Every difference between the file's expectations and the native implementation, each naming its case.
pub fn check(cases: &[Case]) -> Vec<String> {
    let mut diffs = Vec::new();
    for case in cases {
        match case {
            Case::File {
                name,
                length,
                file_hash,
                blocks,
            } => {
                let (got_blocks, got_file) = native_file(*length);
                if got_blocks.len() != blocks.len() {
                    diffs.push(format!(
                        "file {name}: {} blocks expected, {} computed",
                        blocks.len(),
                        got_blocks.len()
                    ));
                }
                for (k, (want, got)) in blocks.iter().zip(&got_blocks).enumerate() {
                    if want != got {
                        diffs.push(format!(
                            "file {name}: block {k} hash expected {want}, got {got}"
                        ));
                    }
                }
                if *file_hash != got_file {
                    diffs.push(format!(
                        "file {name}: file hash expected {file_hash}, got {got_file}"
                    ));
                }
            }
            Case::Oversize { name, length } => {
                let Ok(length) = usize::try_from(*length) else {
                    diffs.push(format!(
                        "oversize {name}: length {length} does not fit usize"
                    ));
                    continue;
                };
                if sync_core_proto::hash_block(&vec![0u8; length]).is_ok() {
                    diffs.push(format!(
                        "oversize {name}: {length} bytes accepted as one block"
                    ));
                }
            }
            Case::ClientKey {
                name,
                device,
                asset,
                file_hash,
                key,
            } => match native_client_key(device, asset, file_hash) {
                Ok(got) if got == *key => {}
                Ok(got) => diffs.push(format!("client_key {name}: expected {key}, got {got}")),
                Err(e) => diffs.push(format!("client_key {name}: {e}")),
            },
            Case::CommitId {
                name,
                unix_ms,
                random,
                id,
            } => match native_commit_id(*unix_ms, random) {
                Ok(got) if got == *id => {}
                Ok(got) => diffs.push(format!("commit_id {name}: expected {id}, got {got}")),
                Err(e) => diffs.push(format!("commit_id {name}: {e}")),
            },
            Case::Planner(planner) => match native_planner(planner) {
                Ok(got) if got == planner.expected => {}
                Ok(got) => diffs.push(format!(
                    "planner {}: expected {}, got {got}",
                    planner.name, planner.expected
                )),
                Err(e) => diffs.push(format!("planner {}: {e}", planner.name)),
            },
        }
    }
    diffs
}

/// `Hash32` from 64 hex characters.
pub fn hash32(text: &str) -> Result<Hash32, String> {
    let bytes: [u8; 32] = unhex(text)?
        .try_into()
        .map_err(|bytes: Vec<u8>| format!("hash of {} bytes", bytes.len()))?;
    Ok(Hash32::from_bytes(bytes))
}
