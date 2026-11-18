//! The target folder `Camera Uploads/<device label>/` (17 §4): found or created through `sync-core`, its node id kept
//! in `cursor`. Upload names and collision handling belong to E-27/c13.

use sync_core::SyncError;
use sync_core_proto::{NodeKind, PageToken};
use uuid::Uuid;

use super::Backup;
use crate::checkpoint::DbError;
use crate::outcome::RunError;

const FOLDER_KEY: &str = "camera_folder";
const CAMERA_UPLOADS: &str = "Camera Uploads";

impl Backup {
    /// The stored folder, or the folder made now. Later runs reuse the stored id without a call.
    pub(super) async fn camera_folder(&self) -> Result<Uuid, RunError> {
        if let Some(text) = self.assets.cursor_text(FOLDER_KEY)? {
            return Uuid::parse_str(&text)
                .map_err(|_| RunError::from(DbError::Malformed("camera folder")));
        }
        let root = self.api.root_node().await?.node_id;
        let camera = self.folder_in(root, CAMERA_UPLOADS).await?;
        let device = self.folder_in(camera, &self.device_label).await?;
        self.assets
            .set_cursor_text(FOLDER_KEY, &device.to_string())?;
        Ok(device)
    }

    /// An existing folder keeps its id (a reinstall, or another process that made it first).
    async fn folder_in(&self, parent: Uuid, name: &str) -> Result<Uuid, RunError> {
        if let Some(found) = self.find_folder(parent, name).await? {
            return Ok(found);
        }
        match self.api.create_folder(parent, name).await {
            Ok(node) => Ok(node.node_id),
            Err(SyncError::NameConflict) => self
                .find_folder(parent, name)
                .await?
                .ok_or(RunError::Folder),
            Err(error) => Err(error.into()),
        }
    }

    async fn find_folder(&self, parent: Uuid, name: &str) -> Result<Option<Uuid>, RunError> {
        let mut after: Option<PageToken> = None;
        loop {
            let page = self.api.list_children(parent, after.as_ref()).await?;
            if let Some(item) = page
                .items
                .iter()
                .find(|item| item.kind == NodeKind::Folder && item.name == name)
            {
                return Ok(Some(item.node_id));
            }
            match page.next {
                Some(next) => after = Some(next),
                None => return Ok(None),
            }
        }
    }
}
