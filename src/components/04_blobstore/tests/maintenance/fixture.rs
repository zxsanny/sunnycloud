//! What the maintenance tests add to the scrub rig: a second master key, a keyring that holds both, and the
//! writer and reader built on it.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use contracts::{
    BlobCatalog as _, BlobRef, BlobRegistry as _, BlobWriter, RegisterState, WrittenBlob,
    WrittenOrPlanned,
};
use sc_db::Scope;
use sunnycloud_blobstore::{
    BlobstoreConfig, FileBlobReader, FileBlobWriter, MasterKeyring, SyncBarrier, blob_path,
};
use uuid::Uuid;

use crate::support::Rig;

pub const KEY_TWO: [u8; 32] = [0x44; 32];

/// A process started with the ring `1=<key file>, 2=<second key file>`, key 2 current.
pub struct Rotated {
    pub keyring: Arc<MasterKeyring>,
    pub writer: Arc<FileBlobWriter>,
    pub reader: FileBlobReader,
    pub config: BlobstoreConfig,
}

impl Rotated {
    pub async fn start(rig: &Rig) -> Rotated {
        let second = rig._scratch.0.join("master2.key");
        fs::write(&second, KEY_TWO).expect("second key file");
        let mut config = rig.config.clone();
        config.master_key_ring = Some(format!(
            "1={},2={}",
            rig.config.master_key_file.expose(),
            second.display()
        ));
        let keyring = Arc::new(MasterKeyring::new(
            config.clone(),
            rig.api.clone(),
            rig.audit.clone(),
        ));
        keyring.load_and_verify().await.expect("rotated keyring");
        let writer = Arc::new(
            FileBlobWriter::new(
                &config,
                Arc::clone(&keyring),
                SyncBarrier::new(Duration::ZERO),
            )
            .expect("writer"),
        );
        let reader = FileBlobReader::new(
            &config,
            Arc::clone(&keyring),
            rig.api.clone(),
            rig.audit.clone(),
            rig.clock.clone(),
        )
        .expect("reader");
        Rotated {
            keyring,
            writer,
            reader,
            config,
        }
    }

    /// A committed one-block blob of `len` bytes of `seed`, wrapped under this process's current key.
    pub async fn store(&self, rig: &Rig, len: usize, seed: u8) -> BlobRef {
        let plan = self.writer.plan(rig.account, len as i64).expect("plan");
        let handle = self
            .writer
            .create_file(plan, len as i64, false)
            .await
            .expect("create_file");
        let token = self
            .writer
            .write_block(&handle, 0, Bytes::from(vec![seed; len]))
            .await
            .expect("block is written");
        self.writer.await_durable(token).await.expect("durable");
        let blob_id = handle.plan.blob_id;
        rig.api
            .run(Scope::Account(rig.account), async |tx| {
                rig.registry
                    .register_in_tx(
                        tx,
                        rig.account,
                        WrittenOrPlanned::Written(WrittenBlob {
                            plan: handle.plan.clone(),
                            size: len as i64,
                        }),
                        RegisterState::Committed,
                    )
                    .await
            })
            .await
            .expect("blob row");
        reference(rig, blob_id).await
    }

    /// The first bytes of the blob, read through the row's current wrap.
    pub async fn head(&self, rig: &Rig, blob_id: Uuid) -> Bytes {
        let blob = reference(rig, blob_id).await;
        contracts::BlobReader::read_head(
            &self.reader,
            blob.blob_id,
            &blob.wrapped_key,
            blob.master_key_id,
            blob.size,
            4096,
        )
        .await
        .expect("blob is readable")
    }
}

pub async fn reference(rig: &Rig, blob_id: Uuid) -> BlobRef {
    rig.catalog
        .get_ref(rig.account, blob_id)
        .await
        .expect("catalog lookup")
        .expect("committed blob has a reference")
}

/// The whole file of each blob, in the order of `ids`.
pub fn snapshot(rig: &Rig, ids: &[Uuid]) -> Vec<Vec<u8>> {
    ids.iter()
        .map(|id| fs::read(blob_path(&rig.volume, *id)).expect("blob file"))
        .collect()
}

/// The error of opening the blob for a whole-object read, if any.
pub async fn open_error(rig: &Rig, blob: &BlobRef) -> Option<contracts::BlobError> {
    contracts::BlobReader::open(
        &rig.reader,
        blob.blob_id,
        &blob.wrapped_key,
        blob.master_key_id,
        blob.size,
        None,
    )
    .await
    .err()
}
