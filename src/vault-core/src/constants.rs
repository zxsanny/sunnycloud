//! Format and bound constants (19 §3 and §6, `vault_format.md`). Compile-time values, not operator config.
//! The WASM and mobile Argon2 memory caps come from the S7 benchmark (E-29/c02) and are not set here yet.

/// The only format version this build reads and writes.
pub const FORMAT_VERSION: u8 = 1;

/// Argon2id memory floor in KiB (64 MiB); also the default.
pub const KDF_M_COST_MIN_KIB: u32 = 65_536;
/// Argon2id memory cap in KiB on native targets (512 MiB).
pub const KDF_M_COST_NATIVE_CAP_KIB: u32 = 524_288;
pub const KDF_T_COST_MIN: u32 = 3;
pub const KDF_T_COST_MAX: u32 = 10;
/// Lanes are fixed to 1: WASM has no threads (deliberate deviation from RFC 9106's p = 4).
pub const KDF_P_COST: u32 = 1;

pub const DEFAULT_KDF_M_COST_KIB: u32 = KDF_M_COST_MIN_KIB;
pub const DEFAULT_KDF_T_COST: u32 = KDF_T_COST_MIN;
pub const DEFAULT_KDF_P_COST: u32 = KDF_P_COST;

/// Shortest passphrase `check_passphrase` accepts, in characters (19 §3).
pub const PASSPHRASE_MIN_CHARS: usize = 12;

/// Plaintext chunk size bounds in bytes (64 KiB to 4 MiB).
pub const CHUNK_SIZE_MIN: u32 = 64 * 1024;
pub const CHUNK_SIZE_MAX: u32 = 4 * 1024 * 1024;

/// Default of the caller-supplied `max_file_bytes` limit (100 MiB).
pub const DEFAULT_MAX_FILE_BYTES: u64 = 104_857_600;
