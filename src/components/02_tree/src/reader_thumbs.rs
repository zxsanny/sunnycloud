//! The thumbnail state of a listing page (AZ-3718): one `ThumbnailStateSource::states` call per page with the
//! content hashes of its image and video files, made after the page's transaction has closed. Every other item,
//! and every item when no source is wired, keeps `ThumbState::None`.

use std::sync::Arc;

use contracts::{
    NodeKind, NodeView, ThumbState, ThumbnailStateError, ThumbnailStateSource, TreeReaderError,
};
use sc_ids::FileHash;
use uuid::Uuid;

use crate::reader::PgTreeReader;

impl PgTreeReader {
    /// Listings then carry the thumbnail state of their image and video items from `source`.
    pub fn with_thumbnails(mut self, source: Arc<dyn ThumbnailStateSource>) -> Self {
        self.thumbnails = Some(source);
        self
    }

    /// Fills `thumb_state` of the page's media items from one source call; no call without a source or without a
    /// media item on the page.
    pub(crate) async fn fill_thumb_states(
        &self,
        account: Uuid,
        items: &mut [NodeView],
    ) -> Result<(), TreeReaderError> {
        let Some(source) = &self.thumbnails else {
            return Ok(());
        };
        let hashes = media_hashes(items);
        if hashes.is_empty() {
            return Ok(());
        }
        let states = source
            .states(account, &hashes)
            .await
            .map_err(|ThumbnailStateError::Unexpected(fault)| TreeReaderError::Unexpected(fault))?;
        for item in items.iter_mut() {
            if let Some(hash) = media_hash(item) {
                item.thumb_state = states.get(&hash).copied().unwrap_or(ThumbState::None);
            }
        }
        Ok(())
    }
}

/// The content hashes of the page's image and video files, each once, in page order.
fn media_hashes(items: &[NodeView]) -> Vec<FileHash> {
    let mut hashes: Vec<FileHash> = Vec::new();
    for hash in items.iter().filter_map(media_hash) {
        if !hashes.contains(&hash) {
            hashes.push(hash);
        }
    }
    hashes
}

/// The content hash of a file whose sniffed media type is `image/*` or `video/*`.
fn media_hash(item: &NodeView) -> Option<FileHash> {
    let media = item.media_type.as_deref()?;
    let top = media.split('/').next()?;
    let thumbnailed = top.eq_ignore_ascii_case("image") || top.eq_ignore_ascii_case("video");
    (item.kind == NodeKind::File && thumbnailed)
        .then_some(item.content_hash)
        .flatten()
}

#[cfg(test)]
mod tests {
    use contracts::NodeState;
    use sc_clock::Timestamp;

    use super::*;

    fn item(kind: NodeKind, media_type: Option<&str>, seed: u8) -> NodeView {
        NodeView {
            id: Uuid::from_u128(u128::from(seed)),
            parent_id: Some(Uuid::from_u128(1000)),
            kind,
            name: format!("item {seed}"),
            state: NodeState::Live,
            current_revision_id: None,
            size: Some(1),
            content_hash: Some(FileHash::from_bytes([seed; 32])),
            media_type: media_type.map(str::to_owned),
            client_mtime: None,
            created_at: Timestamp::from_unix_micros(0),
            updated_at: Timestamp::from_unix_micros(0),
            seq: 1,
            thumb_state: ThumbState::None,
        }
    }

    #[test]
    fn only_image_and_video_file_hashes_are_asked_each_once() {
        // Arrange
        let items = [
            item(NodeKind::File, Some("image/jpeg"), 1),
            item(NodeKind::File, Some("text/plain"), 2),
            item(NodeKind::File, Some("video/mp4"), 3),
            item(NodeKind::File, None, 4),
            item(NodeKind::Folder, Some("image/png"), 5),
            item(NodeKind::File, Some("application/x-image"), 6),
            item(NodeKind::File, Some("Image/HEIC"), 7),
            item(NodeKind::File, Some("image/jpeg"), 1),
        ];

        // Act
        let hashes = media_hashes(&items);

        // Assert
        let expected: Vec<FileHash> = [1, 3, 7]
            .map(|seed| FileHash::from_bytes([seed; 32]))
            .to_vec();
        assert_eq!(hashes, expected);
    }
}
