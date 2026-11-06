//! AC-1: every chunk sealed by the component equals the output of the pinned `aead-stream` encryptor, which
//! generates the vectors at test time.

use aead_stream::EncryptorBE32;
use aead_stream::aead::Payload;
use aead_stream::aead::array::Array;
use chacha20poly1305::{Key, XChaCha20Poly1305};
use sunnycloud_blobstore::{BlobLayout, CHUNK_SIZE, TAG_LEN};

use crate::support::{KEY_BYTES, blob_id, chunks_of, cipher, other_blob_id, plaintext, prefix};

fn oracle_chunks(data: &[u8]) -> Vec<Vec<u8>> {
    let key = Key::from(KEY_BYTES);
    let mut encryptor = Some(EncryptorBE32::<XChaCha20Poly1305>::new(
        &key,
        &Array::from(prefix()),
    ));
    let chunks = chunks_of(data);
    let aad = blob_id();
    let mut sealed = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let payload = Payload {
            msg: chunk,
            aad: aad.as_bytes(),
        };
        let stream = encryptor.as_mut().expect("encryptor present");
        if i + 1 == chunks.len() {
            let last = encryptor.take().expect("encryptor present");
            sealed.push(
                last.encrypt_last(payload)
                    .expect("oracle seals the last chunk"),
            );
        } else {
            sealed.push(stream.encrypt_next(payload).expect("oracle seals a chunk"));
        }
    }
    sealed
}

fn component_chunks(data: &[u8]) -> Vec<Vec<u8>> {
    let layout = BlobLayout::new(data.len() as u64).expect("layout");
    let cipher = cipher();
    chunks_of(data)
        .iter()
        .enumerate()
        .map(|(i, chunk)| {
            let mut buffer = chunk.to_vec();
            cipher
                .seal_chunk(&layout, i as u64, &mut buffer)
                .expect("component seals the chunk");
            buffer
        })
        .collect()
}

fn assert_equals_oracle(size: usize) -> usize {
    let data = plaintext(size);
    let oracle = oracle_chunks(&data);
    let component = component_chunks(&data);
    assert_eq!(component.len(), oracle.len(), "chunk count for size {size}");
    for (i, (ours, theirs)) in component.iter().zip(&oracle).enumerate() {
        assert_eq!(ours, theirs, "chunk {i} of size {size}");
    }
    oracle.len()
}

#[test]
fn chunks_0_1_and_final_of_a_multi_chunk_file_equal_the_oracle() {
    let chunks = assert_equals_oracle(3 * CHUNK_SIZE as usize + 100);

    assert_eq!(chunks, 4);
}

#[test]
fn a_file_whose_final_chunk_is_full_equals_the_oracle() {
    let chunks = assert_equals_oracle(2 * CHUNK_SIZE as usize);

    assert_eq!(chunks, 2);
}

#[test]
fn a_single_chunk_file_equals_the_oracle() {
    assert_eq!(assert_equals_oracle(1000), 1);
    assert_eq!(assert_equals_oracle(1), 1);
    assert_eq!(assert_equals_oracle(CHUNK_SIZE as usize), 1);
}

#[test]
fn an_empty_file_is_one_tag_only_chunk_equal_to_the_oracle() {
    let chunks = assert_equals_oracle(0);
    let sealed = component_chunks(&[]);

    assert_eq!(chunks, 1);
    assert_eq!(sealed[0].len() as u64, TAG_LEN);
}

#[test]
fn chunks_sealed_by_the_oracle_open_in_the_component() {
    let data = plaintext(2 * CHUNK_SIZE as usize + 7);
    let layout = BlobLayout::new(data.len() as u64).expect("layout");
    let cipher = cipher();

    let opened: Vec<u8> = oracle_chunks(&data)
        .into_iter()
        .enumerate()
        .flat_map(|(i, mut chunk)| {
            cipher
                .open_chunk(&layout, i as u64, &mut chunk)
                .expect("component opens the oracle chunk");
            chunk
        })
        .collect();

    assert_eq!(opened, data);
}

#[test]
fn a_changed_aad_makes_an_oracle_chunk_fail_to_open() {
    let data = plaintext(100);
    let layout = BlobLayout::new(100).expect("layout");
    let mut chunk = oracle_chunks(&data).remove(0);
    let other =
        sunnycloud_blobstore::ChunkCipher::new(&crate::support::key(), prefix(), other_blob_id());

    let result = other.open_chunk(&layout, 0, &mut chunk);

    assert!(matches!(result, Err(contracts::BlobError::Corrupt)));
}
