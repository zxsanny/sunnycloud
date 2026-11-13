//! AZ-3842 AC-72 (IT-08-12, worker side): the holder of a running job reads the whole decrypted original of the
//! job's revision through 04's reader, with its exact length, also when the revision is superseded but retained;
//! anyone else gets `LeaseLost`, and a collected revision is obsolete. Files are published through the real 02
//! `publish` with the real media effect over blobs written by the real 04 writer.

use std::sync::Arc;

use bytes::Bytes;
use contracts::{
    CommitEffect, DerivedKind, EntryResult, NewRevision, PublishEntry, PublishResult, Target,
    TreeCommit, TreeReader, WorkerInput, WorkerJobError,
};
use futures_util::StreamExt;
use media_shared::types::Lane;
use sc_ids::{BLOCK_SIZE, FileHash, Hash32, hash_block, rollup};
use sunnycloud_tree::PgTreeService;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::accept_doubles::CollectingTree;
use crate::accept_support::{Accept, WORKER};
use crate::effect_support::current;
use crate::publish_support::{actor, tree_service};
use crate::queue_support::{LEASE, config};

/// Deterministic bytes that do not compress or repeat by block.
fn content(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_be_bytes()[3]
        })
        .collect()
}

fn hashes(bytes: &[u8]) -> (Vec<Hash32>, FileHash) {
    let blocks: Vec<Hash32> = bytes.chunks(BLOCK_SIZE).map(hash_block).collect();
    let file = rollup(&blocks);
    (blocks, file)
}

async fn read_all(input: WorkerInput) -> (u64, Vec<u8>) {
    let WorkerInput { length, mut body } = input;
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        bytes.extend_from_slice(&chunk.expect("chunk"));
    }
    (length, bytes)
}

fn label(result: &Result<WorkerInput, WorkerJobError>) -> &'static str {
    match result {
        Ok(_) => "Ok",
        Err(WorkerJobError::LeaseLost) => "LeaseLost",
        Err(WorkerJobError::RevisionObsolete) => "RevisionObsolete",
        Err(_) => "other",
    }
}

/// Publishes `bytes` as `target` over a fresh committed blob; returns (node, revision).
async fn publish(
    a: &Accept,
    service: &PgTreeService,
    target: Target,
    bytes: &[u8],
    media_type: &str,
) -> (Uuid, Uuid) {
    let blob_id = a
        .blobs
        .put_bytes(&a.q.fixture, a.q.account, Bytes::copy_from_slice(bytes))
        .await;
    let (blocklist, content_hash) = hashes(bytes);
    let entry = PublishEntry {
        target,
        revision: NewRevision {
            revision_id: Uuid::new_v4(),
            blob_id,
            size: i64::try_from(bytes.len()).expect("size"),
            content_hash,
            blocklist,
            media_type: Some(media_type.to_owned()),
            client_mtime: None,
            client_meta: None,
            reserved_bytes: 0,
            import_hint: false,
        },
        vault: false,
    };
    let effects: Vec<Arc<dyn CommitEffect>> = vec![Arc::new(current(&a.q))];
    let result = service
        .publish(actor(a.q.account), vec![entry], effects, sc_testdb::START)
        .await
        .expect("publish");
    let PublishResult::Published(results) = result else {
        panic!("unexpected replay");
    };
    match &results[..] {
        [
            EntryResult::Published {
                node_id,
                revision_id,
                ..
            },
        ] => (*node_id, *revision_id),
        other => panic!("not published: {other:?}"),
    }
}

fn new_at(root: Uuid, name: &str) -> Target {
    Target::New {
        parent_id: root,
        name: name.to_owned(),
    }
}

#[tokio::test]
async fn the_holder_reads_the_whole_plaintext_of_a_photo_and_a_clip_with_its_exact_length() {
    // Arrange
    let a = Accept::new().await;
    let (service, root) = tree_service(&a.q).await;
    let photo = content(4_000_000, 3);
    let clip = content(12 * 1024 * 1024 + 1_234, 7);
    publish(
        &a,
        &service,
        new_at(root, "photo.jpg"),
        &photo,
        "image/jpeg",
    )
    .await;
    publish(&a, &service, new_at(root, "clip.mp4"), &clip, "video/mp4").await;
    let stills =
        a.q.queue
            .claim(Lane::Image, WORKER)
            .await
            .expect("claim")
            .expect("stills job");
    let video =
        a.q.queue
            .claim(Lane::Video, WORKER)
            .await
            .expect("claim")
            .expect("video job");

    // Act
    let photo_read = read_all(
        a.store
            .open_input(stills.job_id, WORKER)
            .await
            .expect("photo"),
    )
    .await;
    let clip_read = read_all(
        a.store
            .open_input(video.job_id, WORKER)
            .await
            .expect("clip"),
    )
    .await;

    // Assert
    for ((length, bytes), original, claimed) in
        [(photo_read, &photo, stills), (clip_read, &clip, video)]
    {
        assert_eq!(length, u64::try_from(original.len()).expect("len"));
        assert_eq!(bytes.len(), original.len());
        assert_eq!(
            hashes(&bytes).1,
            claimed.content_hash,
            "BLAKE3 roll-up of the bytes"
        );
        assert!(bytes == *original, "the plaintext differs");
    }
    drop(service);
    a.finish().await;
}

#[tokio::test]
async fn a_retained_revision_is_served_and_a_non_holder_a_closed_job_or_an_expired_lease_is_lease_lost()
 {
    // Arrange
    let a = Accept::new().await;
    let (service, root) = tree_service(&a.q).await;
    let first = content(300_000, 11);
    let (node, revision) = publish(
        &a,
        &service,
        new_at(root, "photo.jpg"),
        &first,
        "image/jpeg",
    )
    .await;
    let old_job =
        a.q.queue
            .claim(Lane::Image, WORKER)
            .await
            .expect("claim")
            .expect("stills job");
    let replace = Target::Existing {
        node_id: node,
        base_revision: Some(revision),
    };
    publish(&a, &service, replace, &content(200_000, 12), "image/jpeg").await;
    let new_job =
        a.q.queue
            .claim(Lane::Image, WORKER)
            .await
            .expect("claim")
            .expect("new stills job");

    // Act
    let retained = read_all(
        a.store
            .open_input(old_job.job_id, WORKER)
            .await
            .expect("retained"),
    )
    .await;
    let stranger = a.store.open_input(old_job.job_id, "w2").await;
    a.post_webp(old_job.job_id, DerivedKind::Preview).await;
    a.post_webp(old_job.job_id, DerivedKind::Thumb).await;
    let closed = a.store.open_input(old_job.job_id, WORKER).await;
    a.q.clock.tick(LEASE);
    let expired = a.store.open_input(new_job.job_id, WORKER).await;

    // Assert
    assert_eq!(retained.0, 300_000);
    assert!(retained.1 == first, "the superseded revision's plaintext");
    assert_eq!(
        [label(&stranger), label(&closed), label(&expired)],
        ["LeaseLost", "LeaseLost", "LeaseLost"]
    );
    drop(service);
    a.finish().await;
}

#[tokio::test]
async fn an_input_whose_revision_is_collected_after_the_holder_check_is_obsolete_and_before_it_is_lease_lost()
 {
    // Arrange
    let mut collecting: Option<Arc<CollectingTree>> = None;
    let a = Accept::with(
        config(),
        |tree| {
            let wrapped = Arc::new(CollectingTree {
                inner: tree,
                owner: Mutex::new(None),
            });
            collecting = Some(wrapped.clone());
            wrapped as Arc<dyn TreeReader>
        },
        |store| store.registry.clone(),
    )
    .await;
    let collecting = collecting.expect("the tree wrapper");
    let job = a.claimed(media_shared::types::JobKind::Stills, 40).await;
    collecting.arm(a.q.fixture.owner_conn().await).await;

    // Act
    let during = a.store.open_input(job, WORKER).await;
    let after = a.store.open_input(job, WORKER).await;

    // Assert
    assert_eq!(
        [label(&during), label(&after)],
        ["RevisionObsolete", "LeaseLost"]
    );
    collecting.close().await;
    drop(collecting);
    a.finish().await;
}
