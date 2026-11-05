#![allow(dead_code)]

use sc_ids::{FileHash, Hash32};

pub fn hex32(text: &str) -> [u8; 32] {
    assert_eq!(text.len(), 64, "hash must be 64 hex characters: {text}");
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).expect("hex is ascii");
        *slot = u8::from_str_radix(pair, 16).expect("valid hex");
    }
    out
}

pub fn hash32(text: &str) -> Hash32 {
    Hash32::from_bytes(hex32(text))
}

pub fn file_hash(text: &str) -> FileHash {
    FileHash::from_bytes(hex32(text))
}

/// Seeded xorshift generator: the tests need reproducible noise, not randomness.
pub struct Noise(pub u64);

impl Noise {
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}
