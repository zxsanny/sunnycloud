//! Vault membership on the real rig (AZ-3927): a vault is created by the real 20 `PgVaultService` with the rig's
//! `PgTreeService` as its `VaultRoots`, files reach it through 03's session path (`commit_upload`) and batch path
//! (`commit_batch`), and the 08 `MediaEnqueueEffect` plans their jobs. Membership is never written by hand.

use std::sync::Arc;

use contracts::{
    BatchEntry, BatchEntryResult, CommitUpload, Committed, CreatedVault, StartUpload,
    StartUploadOutcome, VaultError, VaultService,
};
use sc_audit::AuditWriter;
use sc_clock::Clock;
use sc_ids::{Hash32, hash_block};
use sunnycloud_vault_service::{PgVaultService, VaultConfig};
use uuid::Uuid;

use crate::real_ports::body;
use crate::real_support::Real;

/// `max_vault_file_bytes` of the rig's 03 (`real_support::transfer_config`).
pub const VAULT_LIMIT: u64 = 104_857_600;

/// An MP4 head (an `ftyp` box of brand `isom`) followed by `seed`: 03 sniffs it as `video/mp4`.
pub fn mp4(seed: &str) -> Vec<u8> {
    let mut data = b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2".to_vec();
    data.extend_from_slice(seed.as_bytes());
    data
}

/// One revision of a file, as stored: id, `client_meta` as text and blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRevision {
    pub id: Uuid,
    pub client_meta: Option<String>,
    pub blob_id: Uuid,
}

impl Real {
    /// A vault rooted at `folder`, created through 20; returns the vault id.
    pub async fn create_vault(&self, folder: Uuid) -> Uuid {
        self.try_create_vault(folder)
            .await
            .expect("create vault")
            .vault_id
    }

    /// 20's `create_vault` on `folder`, with the rig's `PgTreeService` as its `VaultRoots`.
    pub async fn try_create_vault(&self, folder: Uuid) -> Result<CreatedVault, VaultError> {
        let clock: Arc<dyn Clock> = sc_testdb::clock_at_start();
        let vaults = PgVaultService::new(
            self.fixture.api.clone(),
            self.service.clone(),
            clock.clone(),
            AuditWriter::new(self.fixture.api.clone(), clock),
            &VaultConfig {
                max_vault_file_bytes: VAULT_LIMIT,
                max_vault_header_bytes: 1024,
            },
        );
        vaults
            .create_vault(self.account, b"opaque header", folder)
            .await
    }

    /// A folder created through 02's `create_folder`.
    pub async fn make_folder(&self, parent: Uuid, name: &str) -> Uuid {
        self.service
            .create_folder(self.account, parent, name)
            .await
            .expect("create folder")
            .id
    }

    /// `data` (at most one block) committed through the session path as `name` under `parent`, on `base` when
    /// given, with `client_meta`.
    pub async fn commit_to(
        &self,
        parent: Uuid,
        name: &str,
        data: &[u8],
        base: Option<Uuid>,
        client_meta: Option<&str>,
    ) -> Committed {
        let started = self
            .upload
            .start_upload(
                self.account,
                StartUpload {
                    parent_id: parent,
                    name: name.to_owned(),
                    declared_size: data.len() as i64,
                    client_key: Uuid::new_v4().to_string(),
                    content_hash: None,
                    base_revision: base,
                    client_meta: None,
                },
            )
            .await
            .expect("start upload");
        let StartUploadOutcome::Started(started) = started else {
            panic!("expected a started session, got {started:?}");
        };
        let block: Hash32 = hash_block(data);
        self.upload
            .put_block(
                self.account,
                started.upload_id,
                0,
                block,
                body(data),
                data.len() as u64,
            )
            .await
            .expect("put block");
        self.commit_upload(CommitUpload {
            upload_id: started.upload_id,
            parent_id: parent,
            name: name.to_owned(),
            base_revision: base,
            blocklist: vec![block],
            client_mtime: None,
            client_meta: client_meta.map(str::to_owned),
            import_hint: false,
        })
        .await
        .expect("commit upload")
    }

    /// Each `(parent, name, data)` staged and committed in one batch; returns the committed entries in order.
    pub async fn batch_to(&self, files: &[(Uuid, &str, &[u8])]) -> Vec<Committed> {
        let mut entries = Vec::with_capacity(files.len());
        for &(parent, name, data) in files {
            entries.push(BatchEntry {
                staged_key: self.stage(data).await,
                parent_id: parent,
                name: name.to_owned(),
                base_revision: None,
                client_mtime: None,
                client_meta: None,
                import_hint: false,
            });
        }
        let result = self
            .commit_batch(Uuid::new_v4(), entries)
            .await
            .expect("commit batch");
        result
            .per_entry
            .into_iter()
            .map(|entry| match entry {
                BatchEntryResult::Committed(committed) => committed,
                other => panic!("expected Committed, got {other:?}"),
            })
            .collect()
    }

    /// The vault a node is in, read past the service.
    pub async fn vault_of(&self, node: Uuid) -> Option<Uuid> {
        let mut admin = self.fixture.db.admin().await;
        let vault = sqlx::query_scalar("SELECT vault_id FROM nodes WHERE id = $1")
            .bind(node)
            .fetch_one(&mut admin)
            .await
            .expect("read vault_id");
        crate::mutation_support::close(admin).await;
        vault
    }

    /// The parent of a node, read past the service.
    pub async fn parent_of(&self, node: Uuid) -> Option<Uuid> {
        let mut admin = self.fixture.db.admin().await;
        let parent = sqlx::query_scalar("SELECT parent_id FROM nodes WHERE id = $1")
            .bind(node)
            .fetch_one(&mut admin)
            .await
            .expect("read parent_id");
        crate::mutation_support::close(admin).await;
        parent
    }

    /// The revisions of a node in id order, read past the service.
    pub async fn revisions_of(&self, node: Uuid) -> Vec<StoredRevision> {
        let mut admin = self.fixture.db.admin().await;
        let rows: Vec<(Uuid, Option<String>, Uuid)> = sqlx::query_as(
            "SELECT id, client_meta::text, blob_id FROM revisions WHERE node_id = $1 ORDER BY id",
        )
        .bind(node)
        .fetch_all(&mut admin)
        .await
        .expect("read revisions");
        crate::mutation_support::close(admin).await;
        rows.into_iter()
            .map(|(id, client_meta, blob_id)| StoredRevision {
                id,
                client_meta,
                blob_id,
            })
            .collect()
    }

    /// Job rows planned for one revision.
    pub async fn jobs_for(&self, revision: Uuid) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM jobs WHERE revision_id = '{revision}'"
        ))
        .await
    }

    /// The `vault` flag the effects saw for `revision`; panics when no entry carried it.
    pub fn seen_vault(&self, revision: Uuid) -> bool {
        self.faults
            .seen()
            .into_iter()
            .rev()
            .find(|(seen, _)| *seen == revision)
            .map(|(_, vault)| vault)
            .unwrap_or_else(|| panic!("no entry of revision {revision} reached the effects"))
    }
}
