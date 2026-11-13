//! The real 04 blob store over the media test database and a scratch volume: a derived file is written through
//! the writer and registered committed, as the output acceptance will store it, and read back through the reader
//! from the reference 08 hands out. Wired as 04's `tests/reader/support.rs` wires it.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use contracts::{
    BlobReader, BlobRef, BlobRegistry as _, BlobWriter, ByteStream, RegisterState, WrittenOrPlanned,
};
use futures_util::StreamExt;
use sc_audit::AuditWriter;
use sc_config::Secret;
use sc_db::Scope;
use sunnycloud_blobstore::{
    BlobRegistry, BlobstoreConfig, FileBlobReader, FileBlobWriter, MasterKeyring, SyncBarrier,
};
use uuid::Uuid;

use crate::support::{Fixture, load_settings};

/// A 1x1 lossless WebP.
pub const WEBP: [u8; 34] = [
    0x52, 0x49, 0x46, 0x46, 0x1a, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38, 0x4c,
    0x0d, 0x00, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x00, 0x10, 0x07, 0x10, 0x11, 0x11, 0x88, 0x88, 0xfe,
    0x07, 0x00,
];

const KEY: [u8; 32] = [0x22; 32];

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("scratch {} not removed: {error}", self.0.display());
        }
    }
}

pub struct Store {
    writer: FileBlobWriter,
    reader: FileBlobReader,
    registry: BlobRegistry,
    _scratch: Scratch,
}

impl Store {
    pub async fn new(fixture: &Fixture) -> Store {
        let dir = std::env::temp_dir().join(format!("sc-media-blobs-{}", Uuid::new_v4()));
        let volume = dir.join("volume");
        fs::create_dir_all(&volume).expect("volume folder");
        let scratch = Scratch(dir);
        let key_file = scratch.0.join("master.key");
        fs::write(&key_file, KEY).expect("key file");
        let config = BlobstoreConfig {
            blob_root: Some(volume.to_string_lossy().into_owned()),
            blob_sync_coalesce_delay: Duration::ZERO,
            blob_preallocate: false,
            master_key_file: Secret::new(key_file.to_string_lossy().into_owned()),
            master_key_ring: None,
            master_key_current: None,
            ..BlobstoreConfig::from_settings(&load_settings(&[]).expect("settings"))
        };
        let clock = sc_testdb::clock_at_start();
        let audit = AuditWriter::new(fixture.api.clone(), clock.clone());
        let keyring = Arc::new(MasterKeyring::new(
            config.clone(),
            fixture.api.clone(),
            audit.clone(),
        ));
        keyring.load_and_verify().await.expect("keyring starts");
        let writer = FileBlobWriter::new(
            &config,
            Arc::clone(&keyring),
            SyncBarrier::new(Duration::ZERO),
        )
        .expect("writer");
        let reader =
            FileBlobReader::new(&config, keyring, fixture.api.clone(), audit, clock.clone())
                .expect("reader");
        let registry = BlobRegistry::new(&config, clock).expect("registry");
        Store {
            writer,
            reader,
            registry,
            _scratch: scratch,
        }
    }

    /// Writes `bytes` as a blob of `account` and registers it committed; returns its id.
    pub async fn put(&self, fixture: &Fixture, account: Uuid, bytes: &'static [u8]) -> Uuid {
        let size = i64::try_from(bytes.len()).expect("size");
        let body: ByteStream =
            Box::pin(futures_util::stream::iter([Ok(Bytes::from_static(bytes))]));
        let written = self
            .writer
            .write_complete(account, size, body)
            .await
            .expect("blob written");
        let blob_id = written.plan.blob_id;
        fixture
            .api
            .run(Scope::Account(account), async |tx| {
                self.registry
                    .register_in_tx(
                        tx,
                        account,
                        WrittenOrPlanned::Written(written),
                        RegisterState::Committed,
                    )
                    .await
            })
            .await
            .expect("blob row");
        blob_id
    }

    /// The whole plaintext of `blob`.
    pub async fn read(&self, blob: &BlobRef) -> Vec<u8> {
        let mut stream = self
            .reader
            .open(
                blob.blob_id,
                &blob.wrapped_key,
                blob.master_key_id,
                blob.size,
                None,
            )
            .await
            .expect("blob opens");
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk.expect("chunk"));
        }
        bytes
    }
}
