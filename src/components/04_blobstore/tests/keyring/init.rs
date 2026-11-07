//! AC-3: `init_key_check` proves the candidate with a sample of committed blobs before it writes anything.

use std::fs;

use contracts::KeyError;
use uuid::Uuid;

use crate::support::{Env, KEY_ONE, OTHER_KEY, candidate};

const INIT_EVENT: &str = "event = 'admin_action' AND detail->>'action' = 'keycheck_init'";

async fn restored_volume(env: &Env, blobs: usize) -> Vec<Uuid> {
    let original = env.keyring_with_key(&KEY_ONE);
    original.load_and_verify().await.expect("original install");
    let rows = env.blob_rows(&original, "committed", blobs).await;
    drop(original);
    fs::remove_file(env.key_check_path()).expect("the volume came back without .keycheck");
    rows
}

#[tokio::test]
async fn a_wrong_key_with_committed_blobs_fails_the_sample_and_writes_nothing() {
    // Arrange
    let env = Env::new().await;
    let rows = restored_volume(&env, 3).await;
    let keyring = env.keyring_with_key(&OTHER_KEY);

    // Act
    let result = keyring.init_key_check(&candidate(&OTHER_KEY)).await;

    // Assert
    match result {
        Err(KeyError::KeyCheckSampleFailed { blob_id }) => assert!(rows.contains(&blob_id)),
        other => panic!("expected KeyCheckSampleFailed, got {other:?}"),
    }
    assert!(!env.key_check_path().exists());
    assert_eq!(env.audit_rows(INIT_EVENT).await, 0);
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn the_sample_names_the_one_blob_the_candidate_cannot_unwrap() {
    // Arrange
    let env = Env::new().await;
    let stranger = Uuid::new_v4();
    let other_key = env.scratch.key_file("other.key", &OTHER_KEY);
    let mut other_config = env.config(&other_key, None, None);
    let other_volume = env.scratch.0.join("other-volume");
    fs::create_dir_all(&other_volume).expect("other volume");
    other_config.blob_root = Some(other_volume.to_string_lossy().into_owned());
    let other = env.keyring(other_config);
    other
        .load_and_verify()
        .await
        .expect("other install on its own volume");
    let key = sunnycloud_blobstore::DataKey::generate().expect("data key");
    let (wrapped, key_id) = other.wrap_data_key(stranger, &key).expect("wrap");
    drop(other);
    let rows = restored_volume(&env, 4).await;
    env.insert_blobs("committed", &[(stranger, wrapped, key_id)])
        .await;
    let keyring = env.keyring_with_key(&KEY_ONE);

    // Act
    let result = keyring.init_key_check(&candidate(&KEY_ONE)).await;

    // Assert
    assert_eq!(rows.len(), 4);
    assert!(matches!(
        result,
        Err(KeyError::KeyCheckSampleFailed { blob_id }) if blob_id == stranger
    ));
    assert!(!env.key_check_path().exists());
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn the_right_key_writes_the_key_check_and_one_admin_action_event() {
    // Arrange
    let env = Env::new().await;
    restored_volume(&env, 3).await;
    let keyring = env.keyring_with_key(&KEY_ONE);

    // Act
    let result = keyring.init_key_check(&candidate(&KEY_ONE)).await;
    let restart = keyring.load_and_verify().await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert!(env.key_check_path().exists());
    assert_eq!(env.audit_rows(INIT_EVENT).await, 1);
    assert_eq!(
        env.audit_rows(&format!("{INIT_EVENT} AND (detail->>'sampled')::int = 3"))
            .await,
        1
    );
    assert!(
        restart.is_ok(),
        "the written check passes the start: {restart:?}"
    );
    let wrong = env.keyring_with_key(&OTHER_KEY);
    assert!(matches!(
        wrong.load_and_verify().await,
        Err(KeyError::KeyCheckMismatch)
    ));
    drop((keyring, wrong));
    env.finish().await;
}

#[tokio::test]
async fn an_empty_blobs_table_skips_the_sample_and_writes() {
    // Arrange
    let env = Env::new().await;
    let keyring = env.keyring_with_key(&KEY_ONE);

    // Act
    let result = keyring.init_key_check(&candidate(&OTHER_KEY)).await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert!(env.key_check_path().exists());
    assert_eq!(
        env.audit_rows(&format!("{INIT_EVENT} AND (detail->>'sampled')::int = 0"))
            .await,
        1
    );
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn blobs_that_are_not_committed_are_not_sampled() {
    // Arrange
    let env = Env::new().await;
    env.insert_blobs("uploading", &[(Uuid::new_v4(), vec![7; 72], 1)])
        .await;
    let keyring = env.keyring_with_key(&KEY_ONE);

    // Act
    let result = keyring.init_key_check(&candidate(&KEY_ONE)).await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert!(env.key_check_path().exists());
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn at_most_100_blobs_are_sampled() {
    // Arrange
    let env = Env::new().await;
    restored_volume(&env, 130).await;
    let keyring = env.keyring_with_key(&KEY_ONE);

    // Act
    let result = keyring.init_key_check(&candidate(&KEY_ONE)).await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        env.audit_rows(&format!("{INIT_EVENT} AND (detail->>'sampled')::int = 100"))
            .await,
        1
    );
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn a_candidate_of_the_wrong_length_writes_nothing() {
    // Arrange
    let env = Env::new().await;
    let keyring = env.keyring_with_key(&KEY_ONE);

    // Act
    let result = keyring.init_key_check(&candidate(&KEY_ONE[..31])).await;

    // Assert
    assert!(matches!(result, Err(KeyError::WrongLength)));
    assert!(!env.key_check_path().exists());
    assert_eq!(env.audit_rows(INIT_EVENT).await, 0);
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn an_existing_key_check_is_never_replaced() {
    // Arrange
    let env = Env::new().await;
    let keyring = env.keyring_with_key(&KEY_ONE);
    keyring.load_and_verify().await.expect("install");
    let stored = fs::read(env.key_check_path()).expect("key check");

    // Act
    let wrong = keyring.init_key_check(&candidate(&OTHER_KEY)).await;
    let right = keyring.init_key_check(&candidate(&KEY_ONE)).await;

    // Assert
    assert!(matches!(wrong, Err(KeyError::KeyCheckMismatch)));
    assert!(right.is_ok());
    assert_eq!(fs::read(env.key_check_path()).expect("key check"), stored);
    assert_eq!(env.audit_rows(INIT_EVENT).await, 0);
    drop(keyring);
    env.finish().await;
}
