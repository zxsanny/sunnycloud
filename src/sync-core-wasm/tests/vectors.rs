//! Native leg of the vector gate (AC-2, AC-3, AC-6): the vector file against `sync-core-proto` native,
//! and the adapter's own Rust (`sync_core_wasm::pure`) against the same file.
//!
//! `SYNC_CORE_VECTORS=<path>` points the runner at another copy (the gate's seeded-difference test).

#[path = "common/vector_file.rs"]
mod vector_file;

use std::collections::BTreeSet;

use sync_core_wasm::{ExportError, pure};
use vector_file::{Case, check, fill_test_bytes, hash32, hex, parse, unhex};

const SC_IDS_VECTORS: &str = include_str!("../../shared/ids_hashing/tests/vectors/hashing.txt");
const MIB: u64 = 1024 * 1024;

fn vector_path() -> String {
    std::env::var("SYNC_CORE_VECTORS").unwrap_or_else(|_unset| {
        format!(
            "{}/{}",
            env!("CARGO_MANIFEST_DIR"),
            vector_file::DEFAULT_PATH
        )
    })
}

fn vector_text() -> String {
    let path = vector_path();
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read vector file {path}: {e}"))
}

fn committed_text() -> String {
    let path = format!(
        "{}/{}",
        env!("CARGO_MANIFEST_DIR"),
        vector_file::DEFAULT_PATH
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read vector file {path}: {e}"))
}

#[test]
fn vector_file_matches_native() {
    // Arrange
    let cases = parse(&vector_text()).expect("vector file parses");

    // Act
    let diffs = check(&cases);

    // Assert
    for diff in &diffs {
        eprintln!("DIFF native {diff}");
    }
    assert!(
        diffs.is_empty(),
        "{} difference(s) against native: {diffs:#?}",
        diffs.len()
    );
}

#[test]
fn vector_file_is_complete_and_under_one_megabyte() {
    // Arrange
    let text = committed_text();

    // Act
    let cases = parse(&text).expect("vector file parses");
    let sizes: BTreeSet<u64> = cases
        .iter()
        .filter_map(|c| match c {
            Case::File { length, .. } => Some(*length),
            _ => None,
        })
        .collect();
    let count = |pick: fn(&Case) -> bool| cases.iter().filter(|c| pick(c)).count();

    // Assert
    assert!(
        text.len() < 1_000_000,
        "vector file is {} bytes",
        text.len()
    );
    let required = [0, 1, 2 * MIB - 1, 2 * MIB, 2 * MIB + 1, 6 * MIB, 100 * MIB];
    assert!(
        required.iter().all(|size| sizes.contains(size)),
        "file sizes {sizes:?} miss one of {required:?}"
    );
    assert!(count(|c| matches!(c, Case::Oversize { length, .. } if *length == 2 * MIB + 1)) >= 1);
    assert!(count(|c| matches!(c, Case::ClientKey { .. })) >= 1);
    assert!(count(|c| matches!(c, Case::CommitId { .. })) >= 1);
    assert!(count(|c| matches!(c, Case::Planner(_))) >= 1);
}

#[test]
fn vector_file_extends_the_sc_ids_vectors() {
    // Arrange
    let text = committed_text();
    let ours: BTreeSet<&str> = text.lines().collect();

    // Act
    let missing: Vec<&str> = SC_IDS_VECTORS
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter(|line| !ours.contains(line))
        .collect();

    // Assert
    assert!(
        missing.is_empty(),
        "sc_ids cases missing or changed: {missing:#?}"
    );
}

#[test]
fn seeded_one_bit_difference_is_named() {
    // Arrange
    let text = committed_text();
    let six_mib = text
        .lines()
        .find(|line| line.starts_with("file six_mib "))
        .expect("six_mib case present");
    let hash = six_mib.split(' ').nth(3).expect("file hash field");
    let first = hash.chars().next().expect("non-empty hash");
    let flipped_digit = format!("{:x}", first.to_digit(16).expect("hex digit") ^ 1);
    let seeded_hash = format!("{flipped_digit}{}", &hash[1..]);
    let seeded = text.replace(six_mib, &six_mib.replace(hash, &seeded_hash));

    // Act
    let diffs = check(&parse(&seeded).expect("seeded file parses"));

    // Assert
    assert_eq!(diffs.len(), 1, "{diffs:#?}");
    assert!(
        diffs[0].starts_with("file six_mib: file hash"),
        "{}",
        diffs[0]
    );
}

/// The adapter's Rust built natively gives the file's bytes too (the WASM legs run the same code).
#[test]
fn adapter_functions_match_the_vector_file() {
    // Arrange
    let cases = parse(&vector_text()).expect("vector file parses");
    let mut diffs = Vec::new();
    let mut block = vec![0u8; sync_core_proto::BLOCK_SIZE];

    // Act
    for case in &cases {
        match case {
            Case::File {
                name,
                length,
                file_hash,
                blocks,
            } if *length <= 6 * MIB => {
                let mut hashes = Vec::new();
                for (k, want) in blocks.iter().enumerate() {
                    let offset = k as u64 * sync_core_proto::BLOCK_SIZE as u64;
                    let len = (*length - offset).min(sync_core_proto::BLOCK_SIZE as u64) as usize;
                    fill_test_bytes(&mut block[..len], offset);
                    let got = pure::hash_block(&block[..len]).expect("block fits");
                    if hex(&got) != *want {
                        diffs.push(format!("file {name}: block {k}"));
                    }
                    hashes.extend_from_slice(&got);
                }
                let file = pure::rollup(&hashes).expect("whole hashes");
                if hex(&file) != *file_hash {
                    diffs.push(format!("file {name}: file hash"));
                }
            }
            Case::Oversize { name, length } => {
                let data = vec![0u8; *length as usize];
                if pure::hash_block(&data) != Err(ExportError::BlockTooLarge) {
                    diffs.push(format!("oversize {name}"));
                }
            }
            Case::ClientKey {
                name,
                device,
                asset,
                file_hash,
                key,
            } => {
                let hash = hash32(file_hash).expect("hash hex");
                match pure::client_key_for_mobile(device, asset, hash.as_bytes()) {
                    Ok(got) if got.to_string() == *key => {}
                    other => diffs.push(format!("client_key {name}: {other:?}")),
                }
            }
            Case::CommitId {
                name,
                unix_ms,
                random,
                id,
            } => {
                let random = unhex(random).expect("random hex");
                match pure::new_commit_id(*unix_ms as f64, &random) {
                    Ok(got) if got.to_string() == *id => {}
                    other => diffs.push(format!("commit_id {name}: {other:?}")),
                }
            }
            Case::File { .. } | Case::Planner(_) => {}
        }
    }

    // Assert
    assert!(diffs.is_empty(), "adapter differs: {diffs:#?}");
}
