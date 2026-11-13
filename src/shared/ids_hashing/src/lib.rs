//! Identifiers, block and file hashing, mobile commit ids and tokens, in one crate so the server, the
//! desktop and mobile cores and the web hasher agree byte for byte.
//!
//! The hashing part ([`hash_block`], [`FileHasher`], [`rollup`], [`derive_client_key`]) is pure and also
//! builds for `wasm32-unknown-unknown`. Tokens and ids draw randomness from the OS, or from
//! `crypto.getRandomValues` in the browser.
//!
//! Frozen for v1: the block size, the rollup context `sunnycloud file v1` and the commit-key context
//! `sunnycloud commit v1`. The shared vector file `tests/vectors/hashing.txt` gates every consumer.

mod error;
mod hashing;
#[cfg(feature = "new-id")]
mod id;
mod token;
mod types;

pub use error::{HashError, RngUnavailable};
pub use hashing::{BLOCK_SIZE, FileHasher, derive_client_key, hash_block, rollup};
#[cfg(feature = "new-id")]
pub use id::new_id;
pub use token::{
    RawToken, TokenKind, UserCode, ct_eq, new_random_text, new_token, new_user_code, sha256_token,
};
pub use types::{FileHash, Hash32, TokenHash};
