//! AC-5: ring parsing, the current key id and the rotation of an installed key check.

use std::fs;

use contracts::KeyError;
use sc_config::{KeyProblem, Reason};
use sunnycloud_blobstore::DataKey;
use uuid::Uuid;

use crate::support::{Env, KEY_ONE, KEY_TWO, OTHER_KEY, ids};

#[tokio::test]
async fn new_wraps_use_the_highest_id_and_old_wraps_still_unwrap() {
    // Arrange
    let env = Env::new().await;
    let old = env.keyring_with_key(&KEY_ONE);
    let blob = Uuid::new_v4();
    let key = DataKey::generate().expect("data key");
    old.load_and_verify().await.expect("install on key 1");
    let (old_wrap, old_id) = old.wrap_data_key(blob, &key).expect("wrap under 1");
    let k1 = env.scratch.key_file("k1.key", &KEY_ONE);
    let k2 = env.scratch.key_file("k2.key", &KEY_TWO);
    let ring = format!("1={},2={}", k1.display(), k2.display());
    let rotated = env.keyring(env.config(&k1, Some(&ring), None));

    // Act
    let view = rotated.load_and_verify().await.expect("rotated start");
    let (new_wrap, new_id) = rotated.wrap_data_key(blob, &key).expect("wrap");
    let from_old = rotated
        .unwrap_data_key(blob, &old_wrap, old_id)
        .expect("unwrap id 1");
    let from_new = rotated
        .unwrap_data_key(blob, &new_wrap, new_id)
        .expect("unwrap id 2");

    // Assert
    assert_eq!(old_id, 1);
    assert_eq!(new_id, 2);
    assert_eq!(ids(&view), (2, vec![1, 2]));
    assert_eq!(from_old.as_bytes(), key.as_bytes());
    assert_eq!(from_new.as_bytes(), key.as_bytes());
    drop((old, rotated));
    env.finish().await;
}

#[tokio::test]
async fn the_current_id_is_the_highest_unless_configured() {
    // Arrange
    let env = Env::new().await;
    let k1 = env.scratch.key_file("k1.key", &KEY_ONE);
    let k2 = env.scratch.key_file("k2.key", &KEY_TWO);
    let k5 = env.scratch.key_file("k5.key", &OTHER_KEY);
    let ring = format!("5={},2={}", k5.display(), k2.display());
    let by_default = env.keyring(env.config(&k1, Some(&ring), None));
    let pinned = env.keyring(env.config(&k1, Some(&ring), Some(2)));

    // Act
    let default_view = by_default.load_and_verify().await.expect("default current");
    let pinned_view = pinned.load_and_verify().await.expect("pinned current");
    let key = DataKey::generate().expect("data key");
    let pinned_wrap = pinned.wrap_data_key(Uuid::new_v4(), &key).expect("wrap");

    // Assert
    assert_eq!(ids(&default_view), (5, vec![1, 2, 5]));
    assert_eq!(ids(&pinned_view), (2, vec![1, 2, 5]));
    assert_eq!(pinned_wrap.1, 2);
    drop((by_default, pinned));
    env.finish().await;
}

#[tokio::test]
async fn a_ring_key_added_to_an_installed_key_check_is_recorded_and_then_enforced() {
    // Arrange
    let env = Env::new().await;
    let k1 = env.scratch.key_file("k1.key", &KEY_ONE);
    let k2 = env.scratch.key_file("k2.key", &KEY_TWO);
    let wrong2 = env.scratch.key_file("wrong2.key", &OTHER_KEY);
    env.keyring(env.config(&k1, None, None))
        .load_and_verify()
        .await
        .expect("install");
    let before = fs::read(env.key_check_path()).expect("key check");
    let with_two = env.keyring(env.config(&k1, Some(&format!("2={}", k2.display())), None));
    let other_two = env.keyring(env.config(&k1, Some(&format!("2={}", wrong2.display())), None));
    let wrong_one = env.keyring(env.config(&wrong2, Some(&format!("2={}", k2.display())), None));

    // Act
    let added = with_two.load_and_verify().await;
    let after = fs::read(env.key_check_path()).expect("key check");
    let swapped_two = other_two.load_and_verify().await;
    let wrong_first = wrong_one.load_and_verify().await;

    // Assert
    assert!(added.is_ok());
    assert!(after.len() > before.len());
    assert!(matches!(swapped_two, Err(KeyError::KeyCheckMismatch)));
    assert!(matches!(wrong_first, Err(KeyError::KeyCheckMismatch)));
    drop((with_two, other_two, wrong_one));
    env.finish().await;
}

#[tokio::test]
async fn a_ring_that_gives_key_one_a_second_file_is_refused() {
    // Arrange
    let env = Env::new().await;
    let k1 = env.scratch.key_file("k1.key", &KEY_ONE);
    let k2 = env.scratch.key_file("k2.key", &KEY_TWO);
    let config = env.config(&k1, Some(&format!("1={}", k2.display())), None);
    let problems = config.validate();
    let keyring = env.keyring(config);

    // Act
    let result = keyring.load_and_verify().await;

    // Assert
    assert_eq!(
        problems,
        vec![KeyProblem::new("master_key_ring", Reason::WrongType)]
    );
    assert!(matches!(result, Err(KeyError::Unexpected(_))));
    assert!(!env.key_check_path().exists());
    drop(keyring);
    env.finish().await;
}
