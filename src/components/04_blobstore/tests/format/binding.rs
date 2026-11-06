//! AC-3: a chunk is bound to its blob id, its index and its last-chunk flag, and a damaged chunk or file
//! shape is `Corrupt`.

use contracts::BlobError;
use sunnycloud_blobstore::{BlobLayout, CHUNK_SIZE, ChunkCipher, FormatError};

use crate::support::{blob_id, cipher, key, other_blob_id, plaintext, prefix};

fn sealed_middle_chunk() -> (Vec<u8>, BlobLayout) {
    let size = 4 * CHUNK_SIZE + 10;
    let layout = BlobLayout::new(size).expect("layout");
    let mut buffer = plaintext(CHUNK_SIZE as usize);
    cipher().seal(1, false, &mut buffer).expect("seal");
    (buffer, layout)
}

#[test]
fn a_chunk_opens_at_its_own_index_and_flag() {
    let (mut sealed, _) = sealed_middle_chunk();

    cipher().open(1, false, &mut sealed).expect("opens");

    assert_eq!(sealed, plaintext(CHUNK_SIZE as usize));
}

#[test]
fn a_chunk_opened_as_another_blob_is_corrupt() {
    let (mut sealed, _) = sealed_middle_chunk();
    let other = ChunkCipher::new(&key(), prefix(), other_blob_id());

    let result = other.open(1, false, &mut sealed);

    assert!(matches!(result, Err(BlobError::Corrupt)));
}

#[test]
fn a_chunk_opened_as_the_next_index_is_corrupt() {
    let (mut sealed, _) = sealed_middle_chunk();

    let result = cipher().open(2, false, &mut sealed);

    assert!(matches!(result, Err(BlobError::Corrupt)));
}

#[test]
fn a_chunk_opened_with_the_flag_flipped_is_corrupt() {
    let (mut sealed, _) = sealed_middle_chunk();
    let mut last = plaintext(10);
    cipher().seal(4, true, &mut last).expect("seal last");

    assert!(matches!(
        cipher().open(1, true, &mut sealed),
        Err(BlobError::Corrupt)
    ));
    assert!(matches!(
        cipher().open(4, false, &mut last),
        Err(BlobError::Corrupt)
    ));
}

#[test]
fn a_chunk_opened_under_another_key_or_prefix_is_corrupt() {
    let (sealed, _) = sealed_middle_chunk();
    let other_key = crate::support::KEY_BYTES.map(|byte| byte ^ 1);
    let mut prefix_changed = prefix();
    prefix_changed[0] ^= 1;

    let wrong_key = ChunkCipher::new(
        &sunnycloud_blobstore::DataKey::from_bytes(&other_key),
        prefix(),
        blob_id(),
    );
    let wrong_prefix = ChunkCipher::new(&key(), prefix_changed, blob_id());

    assert!(matches!(
        wrong_key.open(1, false, &mut sealed.clone()),
        Err(BlobError::Corrupt)
    ));
    assert!(matches!(
        wrong_prefix.open(1, false, &mut sealed.clone()),
        Err(BlobError::Corrupt)
    ));
}

#[test]
fn a_flipped_bit_in_the_ciphertext_or_tag_is_corrupt() {
    let (sealed, _) = sealed_middle_chunk();

    for position in [0, sealed.len() / 2, sealed.len() - 1] {
        let mut damaged = sealed.clone();
        damaged[position] ^= 0x01;

        assert!(
            matches!(
                cipher().open(1, false, &mut damaged),
                Err(BlobError::Corrupt)
            ),
            "bit flipped at {position}"
        );
    }
}

#[test]
fn a_truncated_chunk_is_corrupt_and_never_returns_plaintext() {
    let (sealed, layout) = sealed_middle_chunk();

    for kept in [0, 15, sealed.len() - 1] {
        let mut truncated = sealed[..kept].to_vec();

        assert!(
            matches!(
                cipher().open_chunk(&layout, 1, &mut truncated),
                Err(BlobError::Corrupt)
            ),
            "kept {kept} bytes"
        );
    }
}

#[test]
fn a_chunk_with_trailing_bytes_is_corrupt() {
    let (mut sealed, layout) = sealed_middle_chunk();
    sealed.push(0);

    assert!(matches!(
        cipher().open_chunk(&layout, 1, &mut sealed),
        Err(BlobError::Corrupt)
    ));
}

#[test]
fn the_flag_follows_the_declared_size_so_a_shorter_size_cannot_open_the_chunk() {
    let size = 2 * CHUNK_SIZE;
    let layout = BlobLayout::new(size).expect("layout");
    let shorter = BlobLayout::new(CHUNK_SIZE).expect("layout");
    let mut buffer = plaintext(CHUNK_SIZE as usize);
    cipher().seal_chunk(&layout, 0, &mut buffer).expect("seal");

    assert!(matches!(
        cipher().open_chunk(&shorter, 0, &mut buffer),
        Err(BlobError::Corrupt)
    ));
}

#[test]
fn sealing_a_chunk_of_the_wrong_length_or_past_the_end_is_refused() {
    let layout = BlobLayout::new(CHUNK_SIZE + 5).expect("layout");

    assert_eq!(
        cipher().seal_chunk(&layout, 0, &mut vec![0; 10]),
        Err(FormatError::ChunkLength)
    );
    assert_eq!(
        cipher().seal_chunk(&layout, 1, &mut vec![0; 6]),
        Err(FormatError::ChunkLength)
    );
    assert_eq!(
        cipher().seal_chunk(&layout, 2, &mut vec![0; 5]),
        Err(FormatError::ChunkOutOfRange)
    );
    assert!(matches!(
        cipher().open_chunk(&layout, 2, &mut vec![0; 21]),
        Err(BlobError::OutOfRange)
    ));
}

#[test]
fn two_chunks_of_the_same_plaintext_seal_to_different_ciphertext_and_hide_the_plaintext() {
    let layout = BlobLayout::new(2 * CHUNK_SIZE).expect("layout");
    let plain = vec![7; CHUNK_SIZE as usize];
    let mut first = plain.clone();
    let mut second = plain.clone();

    cipher().seal_chunk(&layout, 0, &mut first).expect("seal");
    cipher().seal_chunk(&layout, 1, &mut second).expect("seal");

    assert_ne!(first, second);
    assert_ne!(first[..plain.len()], plain[..]);
}
