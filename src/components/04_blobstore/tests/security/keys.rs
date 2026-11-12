//! ST-04-2 (AC-6): bytes are never produced with a wrong or missing master key. Another key refuses the start;
//! a ring of another key, or one lacking the blob's key id, opens nothing.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use contracts::{BlobError, BlobRef, KeyError};
use sc_config::Secret;
use sc_testdb::TestDb;
use sunnycloud_blobstore::{
    BlobstoreConfig, FileBlobReader, FileBlobWriter, MasterKeyring, SyncBarrier,
};

use crate::kit::{Read, read_all, store, store_with};
use crate::support::{MIB, Rig};

const OTHER: [u8; 32] = [0x44; 32];
const SECOND: [u8; 32] = [0x55; 32];

fn key_file(rig: &Rig, name: &str, key: &[u8; 32]) -> PathBuf {
    let path = rig._scratch.0.join(name);
    fs::write(&path, key).expect("key file");
    path
}

fn with_key(config: &BlobstoreConfig, file: &std::path::Path) -> BlobstoreConfig {
    BlobstoreConfig {
        master_key_file: Secret::new(file.to_string_lossy().into_owned()),
        ..config.clone()
    }
}

fn reader(rig: &Rig, keyring: Arc<MasterKeyring>) -> FileBlobReader {
    FileBlobReader::new(
        &rig.config,
        keyring,
        rig.api.clone(),
        rig.audit.clone(),
        rig.clock.clone(),
    )
    .expect("reader")
}

fn assert_no_plaintext(read: &Read, what: &str) {
    assert!(
        matches!(read.open, Some(BlobError::KeyUnavailable)),
        "{what}: {read:?}"
    );
    assert!(read.bytes.is_empty(), "{what}: plaintext returned");
}

#[tokio::test(flavor = "multi_thread")]
async fn another_master_key_refuses_the_start_and_its_keyring_opens_nothing() {
    // Arrange
    let rig = Rig::new().await;
    let blob = store(&rig, rig.account, &vec![9; 3 * MIB]).await;
    let other = key_file(&rig, "other.key", &OTHER);
    let keyring = Arc::new(MasterKeyring::new(
        with_key(&rig.config, &other),
        rig.api.clone(),
        rig.audit.clone(),
    ));

    // Act
    let started = keyring.load_and_verify().await;
    let read = read_all(&reader(&rig, keyring), blob.blob_id, &blob).await;

    // Assert
    assert!(
        matches!(started, Err(KeyError::KeyCheckMismatch)),
        "{started:?}"
    );
    assert_no_plaintext(&read, "keyring refused at start");
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_blob_wrapped_under_key_a_opened_while_only_key_b_is_loaded_returns_no_byte() {
    // Arrange
    let rig = Rig::new().await;
    let blob = store(&rig, rig.account, &vec![8; 2 * MIB + 5]).await;
    let other_db = TestDb::clone_template().await;
    let other_api = other_db.api().await;
    let other_volume = rig._scratch.0.join("volume-b");
    fs::create_dir_all(&other_volume).expect("volume b");
    let other = key_file(&rig, "b.key", &OTHER);
    let config_b = BlobstoreConfig {
        blob_root: Some(other_volume.to_string_lossy().into_owned()),
        ..with_key(&rig.config, &other)
    };
    let audit_b = sc_audit::AuditWriter::new(other_api.clone(), rig.clock.clone());
    let keyring_b = Arc::new(MasterKeyring::new(config_b, other_api.clone(), audit_b));
    keyring_b
        .load_and_verify()
        .await
        .expect("key B starts on its own empty volume");

    // Act
    let read = read_all(&reader(&rig, Arc::clone(&keyring_b)), blob.blob_id, &blob).await;

    // Assert
    assert_eq!(blob.master_key_id, 1);
    assert_no_plaintext(&read, "key B as id 1");
    drop((keyring_b, other_api));
    assert_eq!(other_db.finish().await, 0, "connections left open");
    rig.finish().await;
}

/// The rotated blob is wrapped under key 2; a restart whose ring lacks key 2 starts but opens nothing of it.
async fn rotated_blob(rig: &Rig) -> BlobRef {
    let second = key_file(rig, "k2.key", &SECOND);
    let rotated = BlobstoreConfig {
        master_key_ring: Some(format!("2={}", second.display())),
        ..rig.config.clone()
    };
    let keyring = Arc::new(MasterKeyring::new(
        rotated.clone(),
        rig.api.clone(),
        rig.audit.clone(),
    ));
    keyring.load_and_verify().await.expect("rotated start");
    let writer =
        FileBlobWriter::new(&rotated, keyring, SyncBarrier::new(Duration::ZERO)).expect("writer");
    store_with(rig, &writer, rig.account, &vec![7; MIB]).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_ring_lacking_the_blobs_key_id_starts_but_returns_no_byte_of_that_blob() {
    // Arrange
    let rig = Rig::new().await;
    let blob = rotated_blob(&rig).await;
    let restarted = Arc::new(MasterKeyring::new(
        rig.config.clone(),
        rig.api.clone(),
        rig.audit.clone(),
    ));
    let view = restarted
        .load_and_verify()
        .await
        .expect("key 1 alone still passes the key check");

    // Act
    let read = read_all(&reader(&rig, restarted), blob.blob_id, &blob).await;

    // Assert
    assert_eq!(blob.master_key_id, 2);
    assert_eq!(view.key_ids, vec![1]);
    assert_no_plaintext(&read, "ring without key 2");
    rig.finish().await;
}
