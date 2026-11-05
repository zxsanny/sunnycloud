#![allow(dead_code)]

use std::collections::HashSet;

use sc_names::{NameKey, ValidName, name_key, validate_name};

pub fn valid(s: &str) -> ValidName {
    validate_name(s).unwrap_or_else(|e| panic!("{s:?} should be a legal name: {e}"))
}

pub fn key_of(s: &str) -> NameKey {
    name_key(&valid(s))
}

pub fn folder(names: &[&str]) -> HashSet<NameKey> {
    names.iter().map(|n| key_of(n)).collect()
}

/// Seeded xorshift64* generator: reproducible property tests without a dependency.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next_u64() % n as u64).expect("below n fits usize")
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

const POOLS: &[&str] = &[
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
    " .-_()[]%",
    "<>:\"|?*\\",
    "/\0\t\n\u{7f}\u{85}",
    "\u{202e}\u{200f}\u{2066}\u{61c}",
    "\u{e9}\u{c9}\u{df}\u{1e9e}\u{130}\u{131}\u{3a3}\u{3c2}\u{3c3}\u{3b9}\u{3a9}\u{2126}\u{fb01}",
    "\u{300}\u{301}\u{308}\u{323}\u{345}\u{30a}\u{1100}\u{1161}\u{11a8}\u{ac00}",
    "\u{4e2d}\u{6587}\u{3042}\u{30a2}\u{d55c}\u{1f600}\u{1f9d1}\u{200d}\u{fe0f}",
    "\u{10400}\u{10428}\u{1e900}\u{1e922}",
];

pub fn random_string(rng: &mut Rng, max_chars: usize) -> String {
    let len = rng.below(max_chars + 1);
    (0..len)
        .map(|_| {
            let pool: Vec<char> = rng.pick(POOLS).chars().collect();
            rng.pick(&pool)
        })
        .collect()
}

/// A random legal name; falls back to a fixed one when the random draw is rejected.
pub fn random_valid_name(rng: &mut Rng, max_chars: usize) -> ValidName {
    for _ in 0..8 {
        if let Ok(name) = validate_name(&random_string(rng, max_chars)) {
            return name;
        }
    }
    valid("fallback.txt")
}
