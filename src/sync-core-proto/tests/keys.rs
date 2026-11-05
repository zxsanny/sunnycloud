//! AC-5: commit-ID rules (IT-13-17).

mod common;

use std::cell::Cell;

use common::file_hash;
use sync_core_proto::{BatchIdError, check_batch_ids, client_key_for_mobile, reuse_or_new_key};
use uuid::Uuid;

const H: &str = "fe1f7bc3fa20f8b474a44830b7ab8404aa4762c3dbb363d5412164ceb47e6712";
const H2: &str = "5520a63b594dc404b23b16e8067eb38eb17fe84d938325af8c68e791235850ae";
const V7_A: &str = "0190c5f0-0000-7000-8000-00000000000a";
const V7_B: &str = "0190c5f0-0000-7000-8000-00000000000b";

fn uuid(text: &str) -> Uuid {
    text.parse().expect("uuid")
}

#[test]
fn stored_key_is_kept_while_the_hash_is_unchanged() {
    let stored = uuid(V7_A);
    let calls = Cell::new(0);
    let fresh = || {
        calls.set(calls.get() + 1);
        uuid(V7_B)
    };

    let same = reuse_or_new_key(Some((stored, file_hash(H))), file_hash(H), fresh);
    let changed = reuse_or_new_key(Some((stored, file_hash(H))), file_hash(H2), fresh);
    let none = reuse_or_new_key(None, file_hash(H), fresh);

    assert_eq!(same, stored);
    assert_eq!(changed, uuid(V7_B));
    assert_eq!(none, uuid(V7_B));
    assert_eq!(
        calls.get(),
        2,
        "fresh runs only when no matching key is stored"
    );
}

#[test]
fn mobile_key_is_stable_per_triple_and_differs_for_another_hash() {
    let device = uuid("0190c5f0-0000-7000-8000-000000000001");

    let first = client_key_for_mobile(device, "asset-42", &file_hash(H));
    let second = client_key_for_mobile(device, "asset-42", &file_hash(H));
    let other_hash = client_key_for_mobile(device, "asset-42", &file_hash(H2));
    let other_asset = client_key_for_mobile(device, "asset-43", &file_hash(H));

    assert_eq!(first, second);
    assert_ne!(first, other_hash);
    assert_ne!(first, other_asset);
}

#[test]
fn batch_id_must_be_v7_and_equal_no_member_key() {
    let members = [uuid(V7_A), uuid(V7_B)];

    assert_eq!(
        check_batch_ids(uuid(V7_A), &members),
        Err(BatchIdError::EqualsMemberKey)
    );
    assert_eq!(
        check_batch_ids(uuid(V7_B), &members),
        Err(BatchIdError::EqualsMemberKey)
    );
    assert_eq!(
        check_batch_ids(uuid("0190c5f0-0000-7000-8000-0000000000ff"), &members),
        Ok(())
    );
}

#[test]
fn non_v7_batch_ids_are_refused() {
    let members = [uuid(V7_A)];
    let mobile_key = client_key_for_mobile(uuid(V7_B), "asset-42", &file_hash(H));

    assert_eq!(
        check_batch_ids(Uuid::nil(), &members),
        Err(BatchIdError::NotV7)
    );
    assert_eq!(
        check_batch_ids(mobile_key, &members),
        Err(BatchIdError::NotV7)
    );
    assert_eq!(
        check_batch_ids(uuid("0190c5f0-0000-4000-8000-00000000000a"), &[]),
        Err(BatchIdError::NotV7)
    );
}
