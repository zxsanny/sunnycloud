//! The demo content of one seeded user: folders with Unicode and mixed-case names, a committed photo with a
//! share link, and for the first user the 1 GiB limit, for the second an upload session left open.

use bytes::Bytes;
use contracts::{
    Actor, BatchCommit, BatchEntry, BatchEntryResult, ByteStream, CommitActor, DevSeedError,
    StartUpload, StartUploadOutcome,
};
use sc_ids::{hash_block, new_id};
use uuid::Uuid;

use crate::dev_seed::{DevSeedImpl, SeedStepFailed, unexpected};

/// The first folder holds the photo, the second the open upload.
const FOLDERS: [&str; 4] = ["Фото 2026", "Documents", "MiXeD Case", "naïve Café ✓"];
const PHOTO_NAME: &str = "IMG_0001.JPG";
const DEVICE_LABEL: &str = "dev-seed";
/// The limit of the first user, so reservations meet a limit in development.
const SEEDED_LIMIT: i64 = 1 << 30;
const OPEN_UPLOAD_NAME: &str = "Draft Video.MP4";
const OPEN_UPLOAD_SIZE: i64 = 4 << 20;
const OPEN_UPLOAD_KEY: &str = "dev-seed-open-upload";

/// A JPEG head followed by `marker`, so each user's photo is content of its own that 03 sniffs as a JPEG.
fn photo(marker: usize) -> Vec<u8> {
    let mut data = b"\xff\xd8\xff\xe0\x00\x10JFIF\x00".to_vec();
    data.extend_from_slice(format!("dev-seed photo {marker}").as_bytes());
    data
}

fn body(data: Vec<u8>) -> ByteStream {
    Box::pin(futures_util::stream::iter([Ok(Bytes::from(data))]))
}

impl DevSeedImpl {
    pub(crate) async fn content(&self, user: Uuid, index: usize) -> Result<(), DevSeedError> {
        let root = self
            .ports
            .tree_reader
            .root_node(user)
            .await
            .map_err(unexpected)?;
        let mut folders = Vec::with_capacity(FOLDERS.len());
        for name in FOLDERS {
            let folder = self
                .ports
                .tree
                .create_folder(user, root.id, name)
                .await
                .map_err(unexpected)?;
            folders.push(folder.id);
        }
        let photo = self.commit_photo(user, folders[0], index).await?;
        self.ports
            .shares
            .create(user, photo, None, None)
            .await
            .map_err(unexpected)?;
        if index == 0 {
            self.admin
                .set_quota(&Actor::Operator, user, Some(SEEDED_LIMIT))
                .await
                .map_err(unexpected)?;
        } else {
            self.open_upload(user, folders[1]).await?;
        }
        Ok(())
    }

    /// Stages the photo and commits it in a batch of one; returns its node.
    async fn commit_photo(
        &self,
        user: Uuid,
        parent: Uuid,
        index: usize,
    ) -> Result<Uuid, DevSeedError> {
        let data = photo(index);
        let (staged_key, length) = (new_id(self.clock.as_ref()), data.len() as u64);
        let hash = hash_block(&data);
        self.ports
            .upload
            .stage_small(user, staged_key, Some(hash), body(data), length)
            .await
            .map_err(unexpected)?;
        let result = self
            .ports
            .commit
            .commit_batch(
                CommitActor {
                    account_id: user,
                    device_label: DEVICE_LABEL.to_owned(),
                },
                BatchCommit {
                    commit_id: new_id(self.clock.as_ref()),
                    entries: vec![BatchEntry {
                        staged_key,
                        parent_id: parent,
                        name: PHOTO_NAME.to_owned(),
                        base_revision: None,
                        client_mtime: None,
                        client_meta: None,
                        import_hint: false,
                    }],
                },
            )
            .await
            .map_err(unexpected)?;
        match result.per_entry.as_slice() {
            [BatchEntryResult::Committed(committed)] => Ok(committed.node_id),
            _ => Err(unexpected(SeedStepFailed("the photo was not committed"))),
        }
    }

    /// A session that is started and never sent, so the installation shows an open upload.
    async fn open_upload(&self, user: Uuid, parent: Uuid) -> Result<(), DevSeedError> {
        let outcome = self
            .ports
            .upload
            .start_upload(
                user,
                StartUpload {
                    parent_id: parent,
                    name: OPEN_UPLOAD_NAME.to_owned(),
                    declared_size: OPEN_UPLOAD_SIZE,
                    client_key: OPEN_UPLOAD_KEY.to_owned(),
                    content_hash: None,
                    base_revision: None,
                    client_meta: None,
                },
            )
            .await
            .map_err(unexpected)?;
        match outcome {
            StartUploadOutcome::Started(_) => Ok(()),
            StartUploadOutcome::NothingToSend { .. } => Err(unexpected(SeedStepFailed(
                "the open upload found its content already stored",
            ))),
        }
    }
}
