//! The per-blob data key: random, redacted in `Debug`, wiped on drop.

use sunnycloud_blobstore::{DATA_KEY_LEN, DataKey};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::support::KEY_BYTES;

fn zeroizes_on_drop<T: ZeroizeOnDrop>(_: &T) {}

#[test]
fn a_generated_key_is_256_random_bits_and_differs_each_time() {
    let first = DataKey::generate().expect("generate");
    let second = DataKey::generate().expect("generate");

    assert_eq!(first.as_bytes().len(), DATA_KEY_LEN);
    assert_eq!(DATA_KEY_LEN, 32);
    assert_ne!(first.as_bytes(), second.as_bytes());
    assert_ne!(first.as_bytes(), &[0; DATA_KEY_LEN]);
}

#[test]
fn debug_prints_a_redaction_and_no_key_bytes() {
    let key = DataKey::from_bytes(&KEY_BYTES);

    let text = format!("{key:?} {key:#?}");

    assert!(text.contains("redacted"));
    assert!(!text.contains("42"));
    assert!(!text.contains("66"));
}

#[test]
fn the_key_is_wiped_by_zeroize_and_is_marked_to_be_wiped_on_drop() {
    let mut key = DataKey::from_bytes(&KEY_BYTES);
    zeroizes_on_drop(&key);

    key.zeroize();

    assert_eq!(key.as_bytes(), &[0; DATA_KEY_LEN]);
}
