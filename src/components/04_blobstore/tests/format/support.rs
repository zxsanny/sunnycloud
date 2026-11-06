use sunnycloud_blobstore::{CHUNK_SIZE, ChunkCipher, DataKey, NONCE_PREFIX_LEN, NoncePrefix};
use uuid::Uuid;

pub const KEY_BYTES: [u8; 32] = [0x42; 32];

pub fn key() -> DataKey {
    DataKey::from_bytes(&KEY_BYTES)
}

pub fn prefix() -> NoncePrefix {
    let mut prefix = [0; NONCE_PREFIX_LEN];
    for (i, byte) in prefix.iter_mut().enumerate() {
        *byte = 0xA0 + i as u8;
    }
    prefix
}

pub fn blob_id() -> Uuid {
    Uuid::from_u128(0x1111_2222_3333_4444_8555_6666_7777_8888)
}

pub fn other_blob_id() -> Uuid {
    Uuid::from_u128(0x9999_aaaa_bbbb_4ccc_8ddd_eeee_ffff_0000)
}

pub fn cipher() -> ChunkCipher {
    ChunkCipher::new(&key(), prefix(), blob_id())
}

/// Deterministic, non-repeating bytes so a chunk swapped for another is visible.
pub fn plaintext(len: usize) -> Vec<u8> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 32) as u8
        })
        .collect()
}

pub fn chunks_of(data: &[u8]) -> Vec<&[u8]> {
    if data.is_empty() {
        return vec![data];
    }
    data.chunks(CHUNK_SIZE as usize).collect()
}
