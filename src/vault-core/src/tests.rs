use alloc::string::ToString;
use alloc::vec::Vec;

use crate::constants::*;
use crate::{ParamField, RandomSource, VaultError};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// One value of every variant; the exhaustive match in `field_kinds` fails to compile when a variant is added.
fn every_variant() -> Vec<VaultError> {
    alloc::vec![
        VaultError::TooShort,
        VaultError::TooWeak,
        VaultError::Rng,
        VaultError::ParamsOutOfBounds {
            field: ParamField::TCost,
        },
        VaultError::HeaderMalformed,
        VaultError::UnknownVersion,
        VaultError::HeaderAuthFailed,
        VaultError::WrongPassphraseOrTampered,
        VaultError::KeyCommitmentMismatch,
        VaultError::HeaderRolledBack { seen: 1, pinned: 2 },
        VaultError::RecoveryKeyInvalid,
        VaultError::TooLarge {
            limit: DEFAULT_MAX_FILE_BYTES,
        },
        VaultError::ChunkSize,
        VaultError::LengthMismatch,
        VaultError::WrappedKeyRejected,
        VaultError::ChunkAuthFailed { index: 7 },
        VaultError::Truncated,
        VaultError::Reordered,
        VaultError::TrailingData,
    ]
}

#[derive(Debug, PartialEq)]
enum FieldKind {
    None,
    BoundField,
    Counter,
}

/// Binds every field with its declared type, so a secret-typed field (key, passphrase, header bytes) cannot be
/// added to a variant without changing this function.
fn field_kinds(error: &VaultError) -> Vec<FieldKind> {
    fn counter(_: &u64) -> FieldKind {
        FieldKind::Counter
    }
    fn index(_: &u32) -> FieldKind {
        FieldKind::Counter
    }
    fn bound(_: &ParamField) -> FieldKind {
        FieldKind::BoundField
    }
    match error {
        VaultError::ParamsOutOfBounds { field } => alloc::vec![bound(field)],
        VaultError::HeaderRolledBack { seen, pinned } => {
            alloc::vec![counter(seen), counter(pinned)]
        }
        VaultError::TooLarge { limit } => alloc::vec![counter(limit)],
        VaultError::ChunkAuthFailed { index: chunk } => alloc::vec![index(chunk)],
        VaultError::TooShort
        | VaultError::TooWeak
        | VaultError::Rng
        | VaultError::HeaderMalformed
        | VaultError::UnknownVersion
        | VaultError::HeaderAuthFailed
        | VaultError::WrongPassphraseOrTampered
        | VaultError::KeyCommitmentMismatch
        | VaultError::RecoveryKeyInvalid
        | VaultError::ChunkSize
        | VaultError::LengthMismatch
        | VaultError::WrappedKeyRejected
        | VaultError::Truncated
        | VaultError::Reordered
        | VaultError::TrailingData => alloc::vec![FieldKind::None],
    }
}

#[test]
fn error_set_has_the_19_contract_variants_with_distinct_names() {
    // Arrange
    let variants = every_variant();

    // Act
    let mut names: Vec<&str> = variants.iter().map(VaultError::name).collect();
    names.sort_unstable();
    names.dedup();

    // Assert
    assert_eq!(names.len(), 19);
}

#[test]
fn no_variant_carries_a_secret_typed_field() {
    // Arrange
    let variants = every_variant();

    // Act
    let carried: Vec<FieldKind> = variants
        .iter()
        .flat_map(field_kinds)
        .filter(|kind| *kind != FieldKind::None)
        .collect();

    // Assert
    assert_eq!(
        carried,
        alloc::vec![
            FieldKind::BoundField,
            FieldKind::Counter,
            FieldKind::Counter,
            FieldKind::Counter,
            FieldKind::Counter,
        ]
    );
}

#[test]
fn display_names_only_non_secret_fields() {
    // Arrange
    let rolled_back = VaultError::HeaderRolledBack { seen: 1, pinned: 2 };
    let bound = VaultError::ParamsOutOfBounds {
        field: ParamField::MCost,
    };
    let too_large = VaultError::TooLarge {
        limit: DEFAULT_MAX_FILE_BYTES,
    };

    // Act
    let texts = [
        rolled_back.to_string(),
        bound.to_string(),
        too_large.to_string(),
    ];

    // Assert
    assert_eq!(
        texts[0],
        "header generation 1 is older than the pinned generation 2"
    );
    assert_eq!(texts[1], "parameter m_cost is out of bounds");
    assert_eq!(texts[2], "file exceeds the limit of 104857600 bytes");
}

#[test]
fn format_and_bound_constants_match_the_contract() {
    // Arrange
    let mib_in_kib = 1024u32;

    // Act
    let kdf = (
        KDF_M_COST_MIN_KIB / mib_in_kib,
        KDF_M_COST_NATIVE_CAP_KIB / mib_in_kib,
        KDF_T_COST_MIN,
        KDF_T_COST_MAX,
        KDF_P_COST,
    );
    let defaults = (
        DEFAULT_KDF_M_COST_KIB,
        DEFAULT_KDF_T_COST,
        DEFAULT_KDF_P_COST,
    );

    // Assert
    assert_eq!(FORMAT_VERSION, 1);
    assert_eq!(kdf, (64, 512, 3, 10, 1));
    assert_eq!(defaults, (65_536, 3, 1));
    assert_eq!((CHUNK_SIZE_MIN, CHUNK_SIZE_MAX), (65_536, 4_194_304));
    assert_eq!(DEFAULT_MAX_FILE_BYTES, 100 * 1024 * 1024);
}

struct FailingSource;

impl RandomSource for FailingSource {
    fn fill(&mut self, _dest: &mut [u8]) -> Result<(), VaultError> {
        Err(VaultError::Rng)
    }
}

struct CountingSource(u8);

impl RandomSource for CountingSource {
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), VaultError> {
        for byte in dest.iter_mut() {
            *byte = self.0;
            self.0 = self.0.wrapping_add(1);
        }
        Ok(())
    }
}

fn draw(source: &mut dyn RandomSource, len: usize) -> Result<Vec<u8>, VaultError> {
    let mut buffer = alloc::vec![0u8; len];
    source.fill(&mut buffer)?;
    Ok(buffer)
}

#[test]
fn injected_source_is_usable_as_a_trait_object() {
    // Arrange
    let mut deterministic = CountingSource(5);
    let mut failing = FailingSource;

    // Act
    let drawn = draw(&mut deterministic, 3);
    let failed = draw(&mut failing, 3);

    // Assert
    assert_eq!(drawn, Ok(alloc::vec![5, 6, 7]));
    assert_eq!(failed, Err(VaultError::Rng));
}

/// Variant names and field counts of the `VaultError` line of the contract, e.g. `HeaderRolledBack{seen, pinned}`.
#[cfg(not(target_arch = "wasm32"))]
fn contract_variants(contract: &str) -> Vec<(alloc::string::String, usize)> {
    let line = contract
        .lines()
        .find(|line| line.starts_with("`VaultError` (complete set"))
        .unwrap_or("");
    line.split('`')
        .skip(1)
        .step_by(2)
        .skip(1)
        .map(|token| match token.split_once('{') {
            Some((name, fields)) => (name.to_string(), fields.split(',').count()),
            None => (token.to_string(), 0),
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn error_set_matches_the_contract_file() {
    // Arrange
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../_docs/02_document/contracts/19_vault_core/vault_core_api.md"
    );
    let contract = std::fs::read_to_string(path).expect("vault_core_api.md is readable");

    // Act
    let expected = contract_variants(&contract);
    let actual: Vec<(alloc::string::String, usize)> = every_variant()
        .iter()
        .map(|error| {
            let fields = field_kinds(error)
                .into_iter()
                .filter(|kind| *kind != FieldKind::None)
                .count();
            (error.name().to_string(), fields)
        })
        .collect();

    // Assert
    assert_eq!(actual, expected);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn contract_parser_reports_a_missing_variant() {
    // Arrange
    let seeded = "`VaultError` (complete set for 1.0.0): `TooShort`, `HeaderRolledBack{seen, pinned}`, plus text";

    // Act
    let parsed = contract_variants(seeded);

    // Assert
    assert_eq!(
        parsed,
        alloc::vec![
            ("TooShort".to_string(), 0),
            ("HeaderRolledBack".to_string(), 2)
        ]
    );
}
