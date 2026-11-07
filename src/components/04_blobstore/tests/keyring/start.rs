//! AC-1, AC-2 and AC-72: the start refusals, the first-start key check and the wrong key.

use std::fs;

use contracts::{BlobError, KeyError};
use sunnycloud_blobstore::{DataKey, KEY_OPERATOR_TEXT, operator_message};
use uuid::Uuid;

use crate::support::{Env, KEY_ONE, OTHER_KEY, ids, is_start_refusal};

fn assert_operator_refusal(error: &KeyError) {
    assert!(is_start_refusal(error), "{error:?}");
    assert!(operator_message(error).contains(KEY_OPERATOR_TEXT));
    assert_eq!(
        KEY_OPERATOR_TEXT,
        "data is unreadable without the master key"
    );
}

fn holds_no_key(keyring: &sunnycloud_blobstore::MasterKeyring) -> bool {
    let key = DataKey::generate().expect("data key");
    matches!(
        keyring.wrap_data_key(Uuid::new_v4(), &key),
        Err(BlobError::KeyUnavailable)
    ) && matches!(
        keyring.unwrap_data_key(Uuid::new_v4(), &[0; 72], 1),
        Err(BlobError::KeyUnavailable)
    )
}

#[tokio::test]
async fn a_missing_key_file_is_file_missing() {
    // Arrange
    let env = Env::new().await;
    let absent = env.scratch.0.join("absent.key");
    let keyring = env.keyring(env.config(&absent, None, None));

    // Act
    let result = keyring.load_and_verify().await;

    // Assert
    let error = result.expect_err("no key file");
    assert!(matches!(error, KeyError::FileMissing), "{error:?}");
    assert_operator_refusal(&error);
    assert!(holds_no_key(&keyring));
    assert!(!env.key_check_path().exists());
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn only_a_key_of_exactly_32_bytes_is_accepted() {
    // Arrange
    let env = Env::new().await;
    let mut outcomes = Vec::new();

    // Act
    for len in [0, 31, 32, 33, 64] {
        let key = env
            .scratch
            .key_file(&format!("k{len}.key"), &vec![0x33; len]);
        let keyring = env.keyring(env.config(&key, None, None));
        let result = keyring.load_and_verify().await;
        outcomes.push((
            len,
            result
                .as_ref()
                .map(|_| ())
                .map_err(|e| matches!(e, KeyError::WrongLength)),
        ));
        if let Err(error) = &result {
            assert_operator_refusal(error);
            assert!(holds_no_key(&keyring));
        }
        fs::remove_file(env.key_check_path()).ok();
    }

    // Assert
    assert_eq!(
        outcomes,
        vec![
            (0, Err(true)),
            (31, Err(true)),
            (32, Ok(())),
            (33, Err(true)),
            (64, Err(true)),
        ]
    );
    env.finish().await;
}

#[tokio::test]
async fn another_key_with_a_key_check_present_is_a_mismatch() {
    // Arrange
    let env = Env::new().await;
    let first = env.keyring_with_key(&KEY_ONE);
    first.load_and_verify().await.expect("first start");
    drop(first);
    let other = env.scratch.key_file("other.key", &OTHER_KEY);
    let keyring = env.keyring(env.config(&other, None, None));

    // Act
    let result = keyring.load_and_verify().await;

    // Assert
    let error = result.expect_err("wrong key");
    assert!(matches!(error, KeyError::KeyCheckMismatch), "{error:?}");
    assert_operator_refusal(&error);
    assert!(holds_no_key(&keyring));
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn the_right_key_without_a_key_check_but_with_blobs_is_refused() {
    // Arrange
    let env = Env::new().await;
    let first = env.keyring_with_key(&KEY_ONE);
    first.load_and_verify().await.expect("first start");
    env.blob_rows(&first, "committed", 2).await;
    drop(first);
    fs::remove_file(env.key_check_path()).expect("delete the key check");
    let keyring = env.keyring_with_key(&KEY_ONE);

    // Act
    let result = keyring.load_and_verify().await;

    // Assert
    let error = result.expect_err("no key check");
    assert!(
        matches!(error, KeyError::KeyCheckAbsentButBlobsExist),
        "{error:?}"
    );
    assert_operator_refusal(&error);
    assert!(holds_no_key(&keyring));
    assert!(!env.key_check_path().exists());
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn an_empty_install_creates_the_key_check_and_the_next_start_passes() {
    // Arrange
    let env = Env::new().await;
    fs::create_dir_all(env.volume.join("blobs").join("ab").join("cd")).expect("empty folders");
    let keyring = env.keyring_with_key(&KEY_ONE);

    // Act
    let loaded = keyring.load_and_verify().await.expect("empty install");
    let created = fs::read(env.key_check_path()).expect("key check exists");
    let second = env.keyring_with_key(&KEY_ONE);
    let again = second.load_and_verify().await;

    // Assert
    assert_eq!(ids(&loaded), (1, vec![1]));
    assert!(created.len() > 16);
    assert!(again.is_ok());
    assert_eq!(fs::read(env.key_check_path()).expect("unchanged"), created);
    assert!(!created.windows(KEY_ONE.len()).any(|w| w == KEY_ONE));
    drop((keyring, second));
    env.finish().await;
}

#[tokio::test]
async fn a_blobs_row_alone_blocks_the_creation() {
    // Arrange
    let env = Env::new().await;
    let other_volume = env.keyring_with_key(&KEY_ONE);
    other_volume.load_and_verify().await.expect("seed ring");
    let rows = env.blob_rows(&other_volume, "uploading", 1).await;
    drop(other_volume);
    fs::remove_file(env.key_check_path()).expect("delete the key check");
    let keyring = env.keyring_with_key(&KEY_ONE);

    // Act
    let result = keyring.load_and_verify().await;

    // Assert
    assert_eq!(rows.len(), 1);
    assert!(matches!(result, Err(KeyError::KeyCheckAbsentButBlobsExist)));
    assert!(!env.key_check_path().exists());
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn a_blob_file_alone_blocks_the_creation() {
    // Arrange
    let env = Env::new().await;
    env.blob_file();
    let keyring = env.keyring_with_key(&KEY_ONE);

    // Act
    let result = keyring.load_and_verify().await;

    // Assert
    assert!(matches!(result, Err(KeyError::KeyCheckAbsentButBlobsExist)));
    assert!(!env.key_check_path().exists());
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn a_volume_root_that_does_not_exist_is_not_an_empty_volume() {
    // Arrange
    let env = Env::new().await;
    let key = env.scratch.key_file("master.key", &KEY_ONE);
    let mut config = env.config(&key, None, None);
    config.blob_root = Some(
        env.scratch
            .0
            .join("unmounted")
            .to_string_lossy()
            .into_owned(),
    );
    let keyring = env.keyring(config);

    // Act
    let result = keyring.load_and_verify().await;

    // Assert
    assert!(matches!(result, Err(KeyError::Unexpected(_))), "{result:?}");
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn a_key_check_file_that_is_not_one_cannot_prove_the_key() {
    // Arrange
    let env = Env::new().await;
    fs::write(env.key_check_path(), b"not a key check").expect("garbage");
    let keyring = env.keyring_with_key(&KEY_ONE);

    // Act
    let result = keyring.load_and_verify().await;

    // Assert
    assert!(matches!(result, Err(KeyError::KeyCheckMismatch)));
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn a_different_master_key_leaves_no_keyring_so_no_blob_can_be_opened() {
    // Arrange
    let env = Env::new().await;
    let good = env.keyring_with_key(&KEY_ONE);
    good.load_and_verify().await.expect("install");
    let blob_id = Uuid::new_v4();
    let data_key = DataKey::generate().expect("data key");
    let (wrapped, key_id) = good.wrap_data_key(blob_id, &data_key).expect("wrap");
    let wrong = env.scratch.key_file("wrong.key", &OTHER_KEY);
    let restarted = env.keyring(env.config(&wrong, None, None));

    // Act
    let result = restarted.load_and_verify().await;
    let opened = restarted.unwrap_data_key(blob_id, &wrapped, key_id);

    // Assert
    let error = result.expect_err("different key");
    assert!(matches!(error, KeyError::KeyCheckMismatch));
    assert!(operator_message(&error).contains(KEY_OPERATOR_TEXT));
    assert!(matches!(opened, Err(BlobError::KeyUnavailable)));
    drop((good, restarted));
    env.finish().await;
}
