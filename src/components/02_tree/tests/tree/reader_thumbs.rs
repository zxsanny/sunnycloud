//! AZ-3718 AC-1 to AC-3 (IT-02-16): the thumbnail state of listing items from one `ThumbnailStateSource` call
//! per page with image and video hashes only, and `none` without a source. The source is the fixed per-call map
//! of 02 tests.md; the real 08 source runs in E-13/c16.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use contracts::{
    EntryResult, NodeKind, NodeView, PublishEntry, ThumbState, ThumbnailStateError,
    ThumbnailStateSource, async_trait,
};
use sc_ids::FileHash;
use sunnycloud_tree::PgTreeReader;
use uuid::Uuid;

use crate::mutation_support::Tree;
use crate::publish_support::new_file;
use crate::reader_support::reader;

const IMAGES: usize = 300;
const VIDEOS: usize = 20;
const TEXTS: usize = 80;
const FOLDERS: usize = 10;
const PAGE: u32 = 100;

/// A fixed hash-to-state map that records the hashes of every call.
#[derive(Default)]
struct FixedMap {
    states: HashMap<FileHash, ThumbState>,
    calls: Mutex<Vec<Vec<FileHash>>>,
}

impl FixedMap {
    fn calls(&self) -> Vec<Vec<FileHash>> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl ThumbnailStateSource for FixedMap {
    async fn states(
        &self,
        _account: Uuid,
        hashes: &[FileHash],
    ) -> Result<HashMap<FileHash, ThumbState>, ThumbnailStateError> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(hashes.to_vec());
        Ok(hashes
            .iter()
            .filter_map(|hash| self.states.get(hash).map(|state| (*hash, *state)))
            .collect())
    }
}

fn typed(parent: Uuid, name: &str, blob: Uuid, media_type: &str) -> PublishEntry {
    let mut entry = new_file(parent, name, blob);
    entry.revision.media_type = Some(media_type.to_owned());
    entry
}

/// A folder of 300 images, 20 videos, 80 text files and 10 folders; returns it and the content hash of each
/// file by media type prefix.
async fn media_folder(tree: &Tree) -> (Uuid, HashMap<&'static str, Vec<FileHash>>) {
    let folder = tree
        .service
        .create_folder(tree.account, tree.root, "media")
        .await
        .expect("create folder")
        .id;
    for index in 0..FOLDERS {
        tree.service
            .create_folder(tree.account, folder, &format!("sub {index:02}"))
            .await
            .expect("create subfolder");
    }
    let kinds: Vec<&'static str> = std::iter::repeat_n("image", IMAGES)
        .chain(std::iter::repeat_n("video", VIDEOS))
        .chain(std::iter::repeat_n("text", TEXTS))
        .collect();
    let blobs = tree.blobs(kinds.len()).await;
    let entries: Vec<PublishEntry> = kinds
        .iter()
        .zip(&blobs)
        .enumerate()
        .map(|(index, (kind, blob))| {
            let media = match *kind {
                "image" => "image/jpeg",
                "video" => "video/mp4",
                _ => "text/plain",
            };
            typed(folder, &format!("file {index:04}"), *blob, media)
        })
        .collect();
    let hashes: Vec<FileHash> = entries
        .iter()
        .map(|entry| entry.revision.content_hash)
        .collect();
    let results = tree.publish_with(entries, Vec::new()).await;
    assert!(
        results
            .iter()
            .all(|r| matches!(r, EntryResult::Published { .. }))
    );
    let mut by_kind: HashMap<&'static str, Vec<FileHash>> = HashMap::new();
    for (kind, hash) in kinds.into_iter().zip(hashes) {
        by_kind.entry(kind).or_default().push(hash);
    }
    (folder, by_kind)
}

/// Every page of `folder`, `PAGE` items each.
async fn pages(reader: &PgTreeReader, account: Uuid, folder: Uuid) -> Vec<Vec<NodeView>> {
    let mut pages = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = reader
            .list_children(account, folder, cursor.as_deref(), PAGE)
            .await
            .expect("list page");
        pages.push(page.items);
        match page.next {
            Some(next) => cursor = Some(next),
            None => return pages,
        }
    }
}

fn is_media(item: &NodeView) -> bool {
    item.kind == NodeKind::File
        && item
            .media_type
            .as_deref()
            .is_some_and(|media| media.starts_with("image/") || media.starts_with("video/"))
}

#[tokio::test]
async fn each_page_asks_the_source_once_with_its_image_and_video_hashes_only() {
    // Arrange
    let tree = Tree::new().await;
    let (folder, _) = media_folder(&tree).await;
    let source = Arc::new(FixedMap::default());
    let reader = reader(&tree, 1000).with_thumbnails(source.clone());

    // Act
    let pages = pages(&reader, tree.account, folder).await;

    // Assert
    let expected: Vec<Vec<FileHash>> = pages
        .iter()
        .map(|items| {
            items
                .iter()
                .filter(|item| is_media(item))
                .filter_map(|item| item.content_hash)
                .collect()
        })
        .filter(|hashes: &Vec<FileHash>| !hashes.is_empty())
        .collect();
    assert_eq!(pages.len(), 5, "410 items in pages of 100");
    assert_eq!(
        source.calls(),
        expected,
        "one call per page, media hashes only"
    );
    assert_eq!(
        expected.iter().map(Vec::len).sum::<usize>(),
        IMAGES + VIDEOS
    );
    drop(reader);
    tree.finish().await;
}

#[tokio::test]
async fn mapped_states_land_on_their_items_and_every_other_item_is_none() {
    // Arrange
    let tree = Tree::new().await;
    let (folder, by_kind) = media_folder(&tree).await;
    let (images, videos, texts) = (&by_kind["image"], &by_kind["video"], &by_kind["text"]);
    let states: HashMap<FileHash, ThumbState> = [
        (images[0], ThumbState::Ready),
        (images[1], ThumbState::Pending),
        (videos[0], ThumbState::Failed),
        (texts[0], ThumbState::Ready),
    ]
    .into();
    let source = Arc::new(FixedMap {
        states: states.clone(),
        calls: Mutex::default(),
    });
    let reader = reader(&tree, 1000).with_thumbnails(source.clone());

    // Act
    let items: Vec<NodeView> = pages(&reader, tree.account, folder)
        .await
        .into_iter()
        .flatten()
        .collect();

    // Assert
    let wrong: Vec<(String, ThumbState)> = items
        .iter()
        .filter(|item| {
            let expected = match item.content_hash {
                Some(hash) if is_media(item) => {
                    states.get(&hash).copied().unwrap_or(ThumbState::None)
                }
                _ => ThumbState::None,
            };
            item.thumb_state != expected
        })
        .map(|item| (item.name.clone(), item.thumb_state))
        .collect();
    assert!(wrong.is_empty(), "items with the wrong state: {wrong:?}");
    let asked: HashSet<FileHash> = source.calls().into_iter().flatten().collect();
    assert!(
        !asked.contains(&texts[0]),
        "a text file's hash is never asked"
    );
    assert_eq!(
        items
            .iter()
            .filter(|item| item.thumb_state != ThumbState::None)
            .count(),
        3
    );
    drop(reader);
    tree.finish().await;
}

#[tokio::test]
async fn without_a_source_every_item_is_none() {
    // Arrange
    let tree = Tree::new().await;
    let (folder, _) = media_folder(&tree).await;
    let reader = reader(&tree, 1000);

    // Act
    let items: Vec<NodeView> = pages(&reader, tree.account, folder)
        .await
        .into_iter()
        .flatten()
        .collect();

    // Assert
    assert_eq!(items.len(), IMAGES + VIDEOS + TEXTS + FOLDERS);
    assert!(
        items
            .iter()
            .all(|item| item.thumb_state == ThumbState::None)
    );
    drop(reader);
    tree.finish().await;
}
