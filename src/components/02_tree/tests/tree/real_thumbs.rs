//! AZ-3720 AC-6 (IT-02-16 part, NFT-PERF-10): images committed through 03 get their `stills` jobs from the real
//! 08 effect; a worker claims some of them through the real queue and 08's `DerivedStore` accepts a WebP
//! thumbnail for each. Listing the folder with the real `ThumbnailStates` asks it once per page, with that page's
//! media hashes, and shows `ready` for the derived thumbnails and `pending` for the queued jobs.

use std::collections::HashMap;
use std::sync::Arc;

use contracts::{AcceptOutcome, BatchEntry, DerivedKind, NodeKind, OutputMeta, ThumbState};
use media_shared::types::Lane;
use sc_ids::FileHash;
use sunnycloud_tree::PgTreeReader;
use uuid::Uuid;

use crate::maintenance_support;
use crate::real_ports::{ObservedStates, body, file_hash, jpeg};
use crate::real_support::Real;

const WORKER: &str = "w1";
const PAGE: u32 = 3;
const FILES: usize = 8;
const DERIVED: usize = 4;

/// A 1x1 lossless WebP, as 08's output checks accept it.
const WEBP: [u8; 34] = [
    0x52, 0x49, 0x46, 0x46, 0x1a, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38, 0x4c,
    0x0d, 0x00, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x00, 0x10, 0x07, 0x10, 0x11, 0x11, 0x88, 0x88, 0xfe,
    0x07, 0x00,
];

/// Files `f0` to `f7` of a new folder: every third one (`f2`, `f5`) text, so each page of 3 holds an image.
async fn folder_of_images(real: &Real) -> (Uuid, Vec<FileHash>) {
    let folder = real
        .service
        .create_folder(real.account, real.root, "photos")
        .await
        .expect("create folder")
        .id;
    let mut entries = Vec::new();
    let mut images = Vec::new();
    for index in 0..FILES {
        let data = if index % 3 == 2 {
            format!("text {index}").into_bytes()
        } else {
            let data = jpeg(&format!("image {index}"));
            images.push(file_hash(&data));
            data
        };
        entries.push(BatchEntry {
            staged_key: real.stage(&data).await,
            parent_id: folder,
            name: format!("f{index}"),
            base_revision: None,
            client_mtime: None,
            client_meta: None,
            import_hint: false,
        });
    }
    real.commit_batch(Uuid::new_v4(), entries)
        .await
        .expect("commit batch");
    (folder, images)
}

/// Claims `count` image jobs for the worker and stores a WebP thumbnail for each; returns their hashes.
async fn derive_thumbnails(real: &Real, count: usize) -> Vec<FileHash> {
    let mut derived = Vec::new();
    for _ in 0..count {
        let job = real
            .queue
            .claim(Lane::Image, WORKER)
            .await
            .expect("claim")
            .expect("a queued image job");
        let meta = OutputMeta {
            media_type: "image/webp".to_owned(),
            width: Some(1),
            height: Some(1),
            duration_ms: None,
        };
        let accepted = real
            .store
            .accept_output(
                job.job_id,
                WORKER,
                DerivedKind::Thumb,
                meta,
                body(&WEBP),
                WEBP.len() as u64,
            )
            .await
            .expect("the thumbnail is accepted");
        assert_eq!(accepted, AcceptOutcome::Accepted);
        derived.push(job.content_hash);
    }
    derived
}

#[tokio::test]
async fn each_page_asks_the_real_source_once_and_derived_thumbnails_are_ready() {
    // Arrange
    let real = Real::new().await;
    let (folder, images) = folder_of_images(&real).await;
    let derived = derive_thumbnails(&real, DERIVED).await;
    let source = Arc::new(ObservedStates::new(real.thumbnail_states()));
    let reader = PgTreeReader::new(
        real.fixture.api.clone(),
        &maintenance_support::config(1000, 1000),
    )
    .with_thumbnails(source.clone());

    // Act
    let mut pages = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = reader
            .list_children(real.account, folder, cursor.as_deref(), PAGE)
            .await
            .expect("list page");
        pages.push(page.items);
        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }

    // Assert
    let asked: Vec<Vec<FileHash>> = pages
        .iter()
        .map(|items| {
            items
                .iter()
                .filter(|item| item.kind == NodeKind::File)
                .filter(|item| item.media_type.as_deref() == Some("image/jpeg"))
                .filter_map(|item| item.content_hash)
                .collect()
        })
        .collect();
    assert_eq!(pages.len(), 3, "8 files in pages of 3");
    assert_eq!(source.calls(), asked, "one call per page, its media hashes");
    let states: HashMap<FileHash, ThumbState> = pages
        .iter()
        .flatten()
        .filter_map(|item| item.content_hash.map(|hash| (hash, item.thumb_state)))
        .collect();
    let expected: HashMap<FileHash, ThumbState> = images
        .iter()
        .map(|hash| {
            let state = if derived.contains(hash) {
                ThumbState::Ready
            } else {
                ThumbState::Pending
            };
            (*hash, state)
        })
        .collect();
    let images_only: HashMap<FileHash, ThumbState> = states
        .iter()
        .filter(|(hash, _)| images.contains(hash))
        .map(|(hash, state)| (*hash, *state))
        .collect();
    assert_eq!(images_only, expected);
    assert_eq!(
        states.len() - images_only.len(),
        FILES - images.len(),
        "the text files are listed"
    );
    assert!(
        states
            .iter()
            .filter(|(hash, _)| !images.contains(hash))
            .all(|(_, state)| *state == ThumbState::None)
    );
    drop((reader, source));
    real.finish().await;
}
