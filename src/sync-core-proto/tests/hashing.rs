//! AC-4: hashes equal the `sc_ids` vector file (AC-7); resume at a block boundary.

mod common;

use common::{file_hash, hash32};
use sync_core_proto::{
    BLOCK_SIZE, FileHasher, HashError, ResumableFileHasher, ResumeHashState, client_key_for_mobile,
    hash_block, rollup,
};
use uuid::Uuid;

const VECTORS: &str = include_str!("../../shared/ids_hashing/tests/vectors/hashing.txt");

fn test_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

fn lines(kind: &'static str) -> impl Iterator<Item = Vec<&'static str>> {
    VECTORS
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| line.split(' ').collect::<Vec<_>>())
        .filter(move |fields| fields[0] == kind)
}

fn expected_blocks(list: &str) -> Vec<sync_core_proto::Hash32> {
    match list {
        "-" => Vec::new(),
        list => list.split(',').map(hash32).collect(),
    }
}

#[test]
fn every_file_vector_matches_block_and_file_hashes() {
    let mut cases = 0;
    for fields in lines("file") {
        let [_, name, length, file, blocks] = fields[..] else {
            panic!("malformed file vector: {fields:?}");
        };
        let data = test_bytes(length.parse().expect("length"));
        let expected = expected_blocks(blocks);

        let hashed: Vec<_> = data
            .chunks(BLOCK_SIZE)
            .map(|block| hash_block(block).expect("block fits"))
            .collect();
        let mut hasher = ResumableFileHasher::default();
        for block in data.chunks(BLOCK_SIZE) {
            hasher.update(block).expect("block fits");
            hasher.finish_block();
        }
        let (blocks, rolled) = hasher.finish();

        assert_eq!(hashed, expected, "hash_block of {name}");
        assert_eq!(blocks, expected, "hasher blocks of {name}");
        assert_eq!(rolled, file_hash(file), "file hash of {name}");
        assert_eq!(rollup(&expected), file_hash(file), "rollup of {name}");
        cases += 1;
    }
    assert_eq!(cases, 4);
}

#[test]
fn a_block_of_two_mib_plus_one_is_block_too_large() {
    let mut cases = 0;
    for fields in lines("oversize") {
        let data = test_bytes(fields[2].parse().expect("length"));

        assert_eq!(
            hash_block(&data),
            Err(HashError::BlockTooLarge),
            "{}",
            fields[1]
        );
        assert_eq!(
            FileHasher::new().update(&data),
            Err(HashError::BlockTooLarge)
        );
        assert_eq!(
            ResumableFileHasher::default().update(&data),
            Err(HashError::BlockTooLarge)
        );
        cases += 1;
    }
    assert_eq!(cases, 1);
}

#[test]
fn mobile_key_vectors_match() {
    let mut cases = 0;
    for fields in lines("client_key") {
        let [_, name, device, asset, file, commit] = fields[..] else {
            panic!("malformed client_key vector: {fields:?}");
        };
        let device: Uuid = device.parse().expect("device uuid");
        let expected: Uuid = commit.parse().expect("commit uuid");

        assert_eq!(
            client_key_for_mobile(device, asset, &file_hash(file)),
            expected,
            "{name}"
        );
        cases += 1;
    }
    assert_eq!(cases, 3);
}

#[test]
fn hashing_resumes_at_a_block_boundary() {
    let fields = lines("file")
        .find(|f| f[1] == "block_2mib_plus_1")
        .expect("vector");
    let (file, blocks) = (fields[3], fields[4]);
    let data = test_bytes(BLOCK_SIZE + 1);
    let first = hash_block(&data[..BLOCK_SIZE]).expect("block fits");
    let state = ResumeHashState::from_hashes(vec![first]);
    assert_eq!(state.blocks_done, 1);
    assert_eq!(state.resume_offset(), BLOCK_SIZE as u64);

    let mut hasher = ResumableFileHasher::resume(state);
    hasher.update(&data[BLOCK_SIZE..]).expect("tail fits");
    let (all, rolled) = hasher.finish();

    assert_eq!(all, expected_blocks(blocks));
    assert_eq!(rolled, file_hash(file));
}
