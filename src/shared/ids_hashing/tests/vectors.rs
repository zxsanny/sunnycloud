//! AC-1: the shared known-answer vector file, recorded by an independent BLAKE3 implementation.
//! The headless WASM test and the UniFFI tests read the same file.

use sc_ids::{
    BLOCK_SIZE, FileHash, FileHasher, Hash32, HashError, derive_client_key, hash_block, rollup,
};
use uuid::Uuid;

const VECTORS: &str = include_str!("vectors/hashing.txt");

fn test_bytes(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

fn hex32(text: &str) -> [u8; 32] {
    assert_eq!(text.len(), 64, "hash must be 64 hex characters: {text}");
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).expect("hex is ascii");
        *slot = u8::from_str_radix(pair, 16).expect("valid hex");
    }
    out
}

fn lines() -> impl Iterator<Item = Vec<&'static str>> {
    VECTORS
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| line.split(' ').collect())
}

#[test]
fn file_vectors_match_block_and_file_hashes() {
    let mut cases = 0;
    for fields in lines().filter(|f| f[0] == "file") {
        let [_, name, length, file_hash, block_hashes] = fields[..] else {
            panic!("malformed file vector: {fields:?}");
        };
        let data = test_bytes(length.parse().expect("length"));
        let expected_blocks: Vec<Hash32> = match block_hashes {
            "-" => Vec::new(),
            list => list
                .split(',')
                .map(|h| Hash32::from_bytes(hex32(h)))
                .collect(),
        };

        let mut hasher = FileHasher::new();
        for block in data.chunks(BLOCK_SIZE) {
            hasher.update(block).expect("block fits");
            hasher.finish_block();
        }
        let (blocks, file) = hasher.finish();

        assert_eq!(blocks, expected_blocks, "block hashes of {name}");
        assert_eq!(
            file,
            FileHash::from_bytes(hex32(file_hash)),
            "file hash of {name}"
        );
        assert_eq!(rollup(&blocks), file, "rollup of {name}");
        for (block, expected) in data.chunks(BLOCK_SIZE).zip(&expected_blocks) {
            assert_eq!(hash_block(block), *expected, "hash_block of {name}");
        }
        cases += 1;
    }
    assert_eq!(
        cases, 4,
        "vector file must carry the 0, 1, 2 MiB and 2 MiB + 1 cases"
    );
}

#[test]
fn oversize_vector_is_rejected_as_one_block() {
    let mut cases = 0;
    for fields in lines().filter(|f| f[0] == "oversize") {
        let data = test_bytes(fields[2].parse().expect("length"));
        let mut hasher = FileHasher::new();
        assert_eq!(
            hasher.update(&data),
            Err(HashError::BlockTooLarge),
            "{}",
            fields[1]
        );
        cases += 1;
    }
    assert_eq!(cases, 1);
}

#[test]
fn client_key_vectors_match() {
    let mut cases = 0;
    for fields in lines().filter(|f| f[0] == "client_key") {
        let [_, name, device, asset, file_hash, commit] = fields[..] else {
            panic!("malformed client_key vector: {fields:?}");
        };
        let device: Uuid = device.parse().expect("device uuid");
        let expected: Uuid = commit.parse().expect("commit uuid");
        let file = FileHash::from_bytes(hex32(file_hash));
        assert_eq!(derive_client_key(device, asset, &file), expected, "{name}");
        cases += 1;
    }
    assert_eq!(cases, 3);
}
