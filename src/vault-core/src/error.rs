use core::fmt;

/// A header or file parameter that is checked against its bound before any Argon2 work (19 §3, AC-61).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum ParamField {
    MCost,
    TCost,
    PCost,
    ChunkSize,
}

impl ParamField {
    pub const fn name(self) -> &'static str {
        match self {
            ParamField::MCost => "m_cost",
            ParamField::TCost => "t_cost",
            ParamField::PCost => "p_cost",
            ParamField::ChunkSize => "chunk_size",
        }
    }
}

/// Every failure of the crate (`vault_core_api.md`). No variant carries a secret, a passphrase or a header
/// value beyond the named bound field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum VaultError {
    TooShort,
    TooWeak,
    Rng,
    ParamsOutOfBounds { field: ParamField },
    HeaderMalformed,
    UnknownVersion,
    HeaderAuthFailed,
    WrongPassphraseOrTampered,
    KeyCommitmentMismatch,
    HeaderRolledBack { seen: u64, pinned: u64 },
    RecoveryKeyInvalid,
    TooLarge { limit: u64 },
    ChunkSize,
    LengthMismatch,
    WrappedKeyRejected,
    ChunkAuthFailed { index: u32 },
    Truncated,
    Reordered,
    TrailingData,
}

impl VaultError {
    /// The variant name, as the WASM and UniFFI adapters expose it.
    pub const fn name(&self) -> &'static str {
        match self {
            VaultError::TooShort => "TooShort",
            VaultError::TooWeak => "TooWeak",
            VaultError::Rng => "Rng",
            VaultError::ParamsOutOfBounds { .. } => "ParamsOutOfBounds",
            VaultError::HeaderMalformed => "HeaderMalformed",
            VaultError::UnknownVersion => "UnknownVersion",
            VaultError::HeaderAuthFailed => "HeaderAuthFailed",
            VaultError::WrongPassphraseOrTampered => "WrongPassphraseOrTampered",
            VaultError::KeyCommitmentMismatch => "KeyCommitmentMismatch",
            VaultError::HeaderRolledBack { .. } => "HeaderRolledBack",
            VaultError::RecoveryKeyInvalid => "RecoveryKeyInvalid",
            VaultError::TooLarge { .. } => "TooLarge",
            VaultError::ChunkSize => "ChunkSize",
            VaultError::LengthMismatch => "LengthMismatch",
            VaultError::WrappedKeyRejected => "WrappedKeyRejected",
            VaultError::ChunkAuthFailed { .. } => "ChunkAuthFailed",
            VaultError::Truncated => "Truncated",
            VaultError::Reordered => "Reordered",
            VaultError::TrailingData => "TrailingData",
        }
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::TooShort => write!(
                f,
                "passphrase is shorter than {} characters",
                crate::constants::PASSPHRASE_MIN_CHARS
            ),
            VaultError::TooWeak => f.write_str("passphrase is too weak"),
            VaultError::Rng => f.write_str("random source failed"),
            VaultError::ParamsOutOfBounds { field } => {
                write!(f, "parameter {} is out of bounds", field.name())
            }
            VaultError::HeaderMalformed => f.write_str("vault header is malformed"),
            VaultError::UnknownVersion => f.write_str("unknown format version"),
            VaultError::HeaderAuthFailed => f.write_str("header authentication failed"),
            VaultError::WrongPassphraseOrTampered => {
                f.write_str("wrong passphrase or tampered header")
            }
            VaultError::KeyCommitmentMismatch => f.write_str("key commitment mismatch"),
            VaultError::HeaderRolledBack { seen, pinned } => write!(
                f,
                "header generation {seen} is older than the pinned generation {pinned}"
            ),
            VaultError::RecoveryKeyInvalid => f.write_str("recovery key is invalid"),
            VaultError::TooLarge { limit } => write!(f, "file exceeds the limit of {limit} bytes"),
            VaultError::ChunkSize => f.write_str("chunk has the wrong size"),
            VaultError::LengthMismatch => f.write_str("plaintext length does not match the header"),
            VaultError::WrappedKeyRejected => f.write_str("wrapped file key rejected"),
            VaultError::ChunkAuthFailed { index } => {
                write!(f, "chunk {index} failed authentication")
            }
            VaultError::Truncated => f.write_str("stream is truncated"),
            VaultError::Reordered => f.write_str("stream chunks are reordered"),
            VaultError::TrailingData => f.write_str("data after the final chunk"),
        }
    }
}

impl core::error::Error for VaultError {}
