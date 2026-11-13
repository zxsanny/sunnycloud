//! wasm-bindgen adapter of `sync-core-proto` for the browser transfer Worker (contract
//! `13_sync_core/wasm_exports.md`): block hash, rollup, commit ID, mobile client key and the upload
//! planner. Every export takes at most one block of bytes, performs no I/O, reads no clock and draws no
//! randomness. The shared vector file `vectors/sync_core_vectors.txt` proves native and WASM agree.

mod error;
mod planner;
pub mod pure;

use wasm_bindgen::prelude::*;

pub use error::ExportError;
pub use planner::{
    BlockResultValue, CommitResultValue, JsAction, UploadPlanner, block_result, commit_result,
};

/// Block size in bytes (2 MiB). A function: wasm-bindgen cannot export a constant.
#[wasm_bindgen]
pub fn block_size() -> u32 {
    // 2 MiB fits u32; the conversion is checked in the native tests.
    sync_core_proto::BLOCK_SIZE as u32
}

/// BLAKE3 of one block (32 bytes). Throws `BlockTooLarge` above `block_size()` bytes.
#[wasm_bindgen]
pub fn hash_block(block: &[u8]) -> Result<Vec<u8>, JsValue> {
    Ok(pure::hash_block(block)?.to_vec())
}

/// File hash (32 bytes) over `n x 32` bytes of block hashes; empty input is the zero-length file.
/// Throws `InvalidLength` when the length is not a multiple of 32.
#[wasm_bindgen]
pub fn rollup(block_hashes: &[u8]) -> Result<Vec<u8>, JsValue> {
    Ok(pure::rollup(block_hashes)?.to_vec())
}

/// A random UUIDv7 commit ID (desktop and web form) as lower-case hyphenated text, from the caller's
/// `Date.now()` and 10 bytes of `crypto.getRandomValues`. Throws `InvalidTime` or `InvalidLength`.
#[wasm_bindgen]
pub fn new_commit_id(unix_ms: f64, random: &[u8]) -> Result<String, JsValue> {
    Ok(pure::new_commit_id(unix_ms, random)?.to_string())
}

/// The mobile `client_key` of `(device_id, asset_id, file_hash)` as UUID text. Throws `InvalidUuid` or
/// `InvalidLength`. Exported for the vector gate; the web app itself uses `new_commit_id`.
#[wasm_bindgen]
pub fn client_key_for_mobile(
    device_id: &str,
    asset_id: &str,
    file_hash: &[u8],
) -> Result<String, JsValue> {
    Ok(pure::client_key_for_mobile(device_id, asset_id, file_hash)?.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn block_size_matches_proto() {
        // Arrange
        let proto = sync_core_proto::BLOCK_SIZE;

        // Act
        let exported = super::block_size();

        // Assert
        assert_eq!(usize::try_from(exported).expect("u32 fits usize"), proto);
    }
}
