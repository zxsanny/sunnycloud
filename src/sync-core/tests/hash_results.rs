//! AC-1 of AZ-3961: `hash_file` gives the block hashes and file hash of the shared vector file, for generated
//! sources and for a file on disk, and reports progress in bytes.

mod connect_support;
mod hash_support;
mod request_support;
mod support;
mod tree_support;

use std::sync::Arc;

use hash_support::{
    BLOCK, DiskSource, GeneratedSource, Progress, SpyStore, checkpoint_into, pattern,
};
use support::Script;
use sync_core::{HashedFile, SyncApi};
use sync_core_proto::{Hash32, hash_block, rollup};
use tree_support::connected;

const VECTORS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../shared/ids_hashing/tests/vectors/hashing.txt"
));
const MIB: u64 = 1024 * 1024;

struct VectorFile {
    name: String,
    len: u64,
    file_hash: String,
    block_hashes: Vec<String>,
}

fn vector_files() -> Vec<VectorFile> {
    VECTORS
        .lines()
        .filter(|line| line.starts_with("file "))
        .map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            let [_, name, len, file_hash, blocks] = fields[..] else {
                panic!("malformed vector line: {line}");
            };
            VectorFile {
                name: name.to_owned(),
                len: len.parse().expect("vector length"),
                file_hash: file_hash.to_owned(),
                block_hashes: if blocks == "-" {
                    Vec::new()
                } else {
                    blocks.split(',').map(str::to_owned).collect()
                },
            }
        })
        .collect()
}

async fn hash(api: &impl SyncApi, source: Arc<dyn sync_core::FileSource>) -> HashedFile {
    api.hash_file(
        source,
        None,
        Progress::new(),
        checkpoint_into(&SpyStore::new()),
    )
    .await
    .expect("a stable source is hashed")
}

fn hex(hashes: &[Hash32]) -> Vec<String> {
    hashes.iter().map(ToString::to_string).collect()
}

#[tokio::test]
async fn hash_file_equals_the_shared_vectors_for_every_vector_file() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let vectors = vector_files();

    // Act
    let mut hashed = Vec::new();
    for vector in &vectors {
        hashed.push(hash(&api, Arc::new(GeneratedSource::new(vector.len))).await);
    }

    // Assert
    assert_eq!(vectors.len(), 4, "the vector file holds four file cases");
    for (vector, hashed) in vectors.iter().zip(&hashed) {
        assert_eq!(
            hex(&hashed.block_hashes),
            vector.block_hashes,
            "{}",
            vector.name
        );
        assert_eq!(
            hashed.file_hash.to_string(),
            vector.file_hash,
            "{}",
            vector.name
        );
    }
}

#[tokio::test]
async fn hash_file_matches_block_by_block_hashing_for_the_sizes_around_the_block_edge_and_six_mib()
{
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let sizes = [0, 1, 2 * MIB - 1, 2 * MIB, 2 * MIB + 1, 6 * MIB];

    for size in sizes {
        // Act
        let hashed = hash(&api, Arc::new(GeneratedSource::new(size))).await;

        // Assert
        let data = pattern(size);
        let expected: Vec<Hash32> = data
            .chunks(BLOCK as usize)
            .map(|chunk| hash_block(chunk).expect("a chunk fits a block"))
            .collect();
        assert_eq!(hashed.block_hashes, expected, "blocks of {size}");
        assert_eq!(hashed.file_hash, rollup(&expected), "file hash of {size}");
    }
}

#[tokio::test]
async fn a_zero_byte_file_has_no_blocks_and_the_empty_rollup() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let source = Arc::new(GeneratedSource::new(0));

    // Act
    let hashed = hash(&api, source.clone()).await;

    // Assert
    assert!(hashed.block_hashes.is_empty());
    assert_eq!(hashed.file_hash, rollup(&[]));
    assert!(source.read_offsets().is_empty());
}

#[tokio::test]
async fn a_file_on_disk_hashes_like_the_same_bytes_in_memory() {
    // Arrange
    let (api, _fixture) = connected(Script::new()).await;
    let bytes = pattern(6 * MIB + 17);
    let disk = Arc::new(DiskSource::create("six-mib.bin", &bytes));
    let progress = Progress::new();

    // Act
    let from_disk = api
        .hash_file(
            disk,
            None,
            progress.clone(),
            checkpoint_into(&SpyStore::new()),
        )
        .await
        .expect("a file on disk is hashed");

    // Assert
    let from_memory = hash(&api, Arc::new(GeneratedSource::new(bytes.len() as u64))).await;
    assert_eq!(from_disk, from_memory);
    let total = bytes.len() as u64;
    assert_eq!(
        progress.calls(),
        vec![
            (BLOCK, total),
            (2 * BLOCK, total),
            (3 * BLOCK, total),
            (total, total)
        ]
    );
}
