//! AC-4: a wrapped data key is bound to its blob and to its master key id.

use contracts::BlobError;
use sunnycloud_blobstore::{DATA_KEY_LEN, DataKey, MasterKeyring};
use uuid::Uuid;

use crate::support::{Env, KEY_ONE};

async fn loaded(env: &Env) -> MasterKeyring {
    let keyring = env.keyring_with_key(&KEY_ONE);
    keyring.load_and_verify().await.expect("start");
    keyring
}

#[tokio::test]
async fn a_wrap_unwraps_for_its_own_blob_and_key_id() {
    // Arrange
    let env = Env::new().await;
    let keyring = loaded(&env).await;
    let b1 = Uuid::new_v4();
    let key = DataKey::generate().expect("data key");

    // Act
    let (wrapped, key_id) = keyring.wrap_data_key(b1, &key).expect("wrap");
    let opened = keyring
        .unwrap_data_key(b1, &wrapped, key_id)
        .expect("unwrap");

    // Assert
    assert_eq!(key_id, 1);
    assert_eq!(opened.as_bytes(), key.as_bytes());
    assert_eq!(wrapped.len(), 24 + DATA_KEY_LEN + 16);
    assert!(!wrapped.windows(DATA_KEY_LEN).any(|w| w == key.as_bytes()));
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn a_wrap_of_one_blob_does_not_unwrap_as_another() {
    // Arrange
    let env = Env::new().await;
    let keyring = loaded(&env).await;
    let (b1, b2) = (Uuid::new_v4(), Uuid::new_v4());
    let key = DataKey::generate().expect("data key");
    let (wrapped, key_id) = keyring.wrap_data_key(b1, &key).expect("wrap");

    // Act
    let as_b2 = keyring.unwrap_data_key(b2, &wrapped, key_id);

    // Assert
    assert!(matches!(as_b2, Err(BlobError::KeyUnavailable)));
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn a_key_id_the_ring_does_not_hold_is_key_unavailable() {
    // Arrange
    let env = Env::new().await;
    let keyring = loaded(&env).await;
    let b1 = Uuid::new_v4();
    let key = DataKey::generate().expect("data key");
    let (wrapped, _) = keyring.wrap_data_key(b1, &key).expect("wrap");

    // Act
    let results: Vec<_> = [2, 3, 0, -1, i16::MAX]
        .into_iter()
        .map(|id| keyring.unwrap_data_key(b1, &wrapped, id))
        .collect();

    // Assert
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(BlobError::KeyUnavailable)))
    );
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn damaged_or_short_wrapped_bytes_yield_no_key() {
    // Arrange
    let env = Env::new().await;
    let keyring = loaded(&env).await;
    let b1 = Uuid::new_v4();
    let key = DataKey::generate().expect("data key");
    let (wrapped, key_id) = keyring.wrap_data_key(b1, &key).expect("wrap");
    let mut cases = vec![
        Vec::new(),
        wrapped[..wrapped.len() - 1].to_vec(),
        [wrapped.clone(), vec![0]].concat(),
    ];
    for at in [0, 30, wrapped.len() - 1] {
        let mut flipped = wrapped.clone();
        flipped[at] ^= 1;
        cases.push(flipped);
    }

    // Act
    let results: Vec<_> = cases
        .iter()
        .map(|bytes| keyring.unwrap_data_key(b1, bytes, key_id))
        .collect();

    // Assert
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Err(BlobError::KeyUnavailable)))
    );
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn every_wrap_uses_a_fresh_nonce() {
    // Arrange
    let env = Env::new().await;
    let keyring = loaded(&env).await;
    let (blob, key) = (Uuid::new_v4(), DataKey::generate().expect("data key"));

    // Act
    let first = keyring.wrap_data_key(blob, &key).expect("wrap");
    let second = keyring.wrap_data_key(blob, &key).expect("wrap");

    // Assert
    assert_ne!(first.0, second.0);
    drop(keyring);
    env.finish().await;
}

#[tokio::test]
async fn a_keyring_that_was_never_verified_wraps_and_unwraps_nothing() {
    // Arrange
    let env = Env::new().await;
    let keyring = env.keyring_with_key(&KEY_ONE);
    let key = DataKey::generate().expect("data key");

    // Act
    let wrapped = keyring.wrap_data_key(Uuid::new_v4(), &key);
    let opened = keyring.unwrap_data_key(Uuid::new_v4(), &[0; 72], 1);

    // Assert
    assert!(matches!(wrapped, Err(BlobError::KeyUnavailable)));
    assert!(matches!(opened, Err(BlobError::KeyUnavailable)));
    drop(keyring);
    env.finish().await;
}
