//! AC-5: defaults of the vault section, read and never waited for, and the refusal of a value that is not a
//! positive byte size, naming the key. No database is used.

use sc_config::{ConfigError, KeyProblem, Reason};
use sunnycloud_vault_service::VaultConfig;

use crate::settings::load_settings;

const MIB: u64 = 1 << 20;

fn config_from(extra: &[(&str, &str)]) -> VaultConfig {
    let settings = load_settings(extra)
        .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
    VaultConfig::from_settings(&settings)
        .unwrap_or_else(|error| panic!("the vault section is refused: {error}"))
}

fn refused_keys(extra: &[(&str, &str)]) -> Vec<String> {
    match load_settings(extra) {
        Err(ConfigError::Invalid(problems)) => {
            problems.iter().map(|problem| problem.key.clone()).collect()
        }
        other => panic!("expected a refused start, got {other:?}"),
    }
}

#[test]
fn no_vault_keys_give_the_documented_defaults() {
    // Arrange
    let no_vault_keys = [];

    // Act
    let config = config_from(&no_vault_keys);

    // Assert
    assert_eq!(config.max_vault_file_bytes, 104_857_600);
    assert_eq!(config.max_vault_header_bytes, 1024);
}

#[test]
fn set_keys_replace_the_defaults() {
    // Arrange
    let keys = [
        ("SUNNYCLOUD_MAX_VAULT_FILE_BYTES", "200MiB"),
        ("SUNNYCLOUD_MAX_VAULT_HEADER_BYTES", "2048"),
    ];

    // Act
    let config = config_from(&keys);

    // Assert
    assert_eq!(config.max_vault_file_bytes, 200 * MIB);
    assert_eq!(config.max_vault_header_bytes, 2048);
}

#[test]
fn a_zero_value_is_refused_naming_its_key() {
    // Arrange
    let keys = [
        ("SUNNYCLOUD_MAX_VAULT_FILE_BYTES", "0"),
        ("SUNNYCLOUD_MAX_VAULT_HEADER_BYTES", "0"),
    ];

    let settings = load_settings(&keys)
        .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));

    // Act
    let result = VaultConfig::from_settings(&settings);

    // Assert
    let out_of_range = |key: &str| {
        KeyProblem::new(
            key,
            Reason::OutOfRange {
                min: "1B".to_owned(),
                max: "no ceiling".to_owned(),
            },
        )
    };
    let Err(ConfigError::Invalid(problems)) = result else {
        panic!("expected a refused section, got {result:?}");
    };
    assert_eq!(
        problems,
        [
            out_of_range("max_vault_file_bytes"),
            out_of_range("max_vault_header_bytes"),
        ]
    );
    assert!(problems[0].to_string().starts_with("max_vault_file_bytes"));
    assert!(
        problems[1]
            .to_string()
            .starts_with("max_vault_header_bytes")
    );
}

#[test]
fn a_negative_value_is_refused_at_load_naming_its_key() {
    // Arrange
    let keys = [
        ("SUNNYCLOUD_MAX_VAULT_FILE_BYTES", "-1"),
        ("SUNNYCLOUD_MAX_VAULT_HEADER_BYTES", "-1"),
    ];

    // Act
    let refused = refused_keys(&keys);

    // Assert
    assert_eq!(refused, ["max_vault_file_bytes", "max_vault_header_bytes"]);
}

#[test]
fn a_value_that_is_not_a_size_is_refused_at_load_naming_its_key() {
    // Arrange
    let keys = [
        ("SUNNYCLOUD_MAX_VAULT_FILE_BYTES", "lots"),
        ("SUNNYCLOUD_MAX_VAULT_HEADER_BYTES", "12XB"),
    ];

    // Act
    let refused = refused_keys(&keys);

    // Assert
    assert_eq!(refused, ["max_vault_file_bytes", "max_vault_header_bytes"]);
}
