//! AC-4 (defaults of the sharing section, read and never waited for), AC-5 (IT-06-12: no usable grant key, no
//! start) and the bounds of the other keys. No database is used.

use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Duration;

use sc_config::{ConfigError, Environment, KeyProblem, Reason, Secret};
use sunnycloud_sharing::{ARGON2_PARAMS_KEY, GRANT_KEY_MIN_BYTES, ShareConfig};

use crate::settings::{KeyFile, TEST_GRANT_KEY, load_settings, load_with_key};

const MINUTE: u64 = 60;
const DAY: u64 = 86_400;
const KEY: &str = "share_grant_key";

type Breaker = fn(&mut ShareConfig);

fn config_from(extra: &[(&str, &str)]) -> ShareConfig {
    ShareConfig::from_settings(&load_settings(TEST_GRANT_KEY.as_bytes(), extra))
}

fn valid() -> ShareConfig {
    config_from(&[])
}

fn out_of_range(key: &str, min: &str, max: &str) -> KeyProblem {
    KeyProblem::new(
        key,
        Reason::OutOfRange {
            min: min.to_owned(),
            max: max.to_owned(),
        },
    )
}

/// What a start does with the key file: the loader's refusal turned into a problem, or the validation of the
/// loaded section.
fn start_problems(key: KeyFile<'_>) -> Vec<KeyProblem> {
    let (sources, loaded) = load_with_key(key, Environment::Production, &[]);
    match loaded {
        Ok(settings) => ShareConfig::from_settings(&settings).validate(),
        Err(error) => {
            let problem = ShareConfig::load_refusal(&error, &sources)
                .unwrap_or_else(|| panic!("the refusal does not concern the key: {error}"));
            vec![problem]
        }
    }
}

fn assert_names_the_key(problems: &[KeyProblem]) {
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].key, KEY);
    assert!(problems[0].to_string().starts_with(KEY), "{}", problems[0]);
}

#[test]
fn only_the_grant_key_set_gives_the_documented_defaults() {
    // Arrange
    let no_other_keys = [];

    // Act
    let config = config_from(&no_other_keys);

    // Assert
    assert_eq!(config.default_ttl, Duration::from_secs(7 * DAY));
    assert_eq!(config.max_ttl, Duration::from_secs(365 * DAY));
    assert_eq!(config.lookup_cache, Duration::from_secs(1));
    assert_eq!(config.grant_ttl, Duration::from_secs(10 * MINUTE));
    assert_eq!(config.wrong_password_limit, 10);
    assert_eq!(
        config.wrong_password_window,
        Duration::from_secs(10 * MINUTE)
    );
    assert_eq!(config.invalid_token_limit, 100);
    assert_eq!(config.password_max_concurrency, None);
    assert_eq!(config.argon2_params, None);
    assert_eq!(config.grant_key.expose(), TEST_GRANT_KEY);
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn set_keys_replace_the_defaults() {
    // Arrange
    let keys = [
        ("SUNNYCLOUD_SHARE_DEFAULT_TTL", "1d"),
        ("SUNNYCLOUD_SHARE_WRONG_PASSWORD_LIMIT", "3"),
        ("SUNNYCLOUD_SHARE_PASSWORD_MAX_CONCURRENCY", "4"),
        ("SUNNYCLOUD_SHARE_ARGON2_PARAMS", "19456,2,1"),
    ];

    // Act
    let config = config_from(&keys);

    // Assert
    assert_eq!(config.default_ttl, Duration::from_secs(DAY));
    assert_eq!(config.wrong_password_limit, 3);
    assert_eq!(config.password_max_concurrency, Some(4));
    assert_eq!(config.argon2_params.as_deref(), Some("19456,2,1"));
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn a_missing_key_file_is_refused_naming_the_key() {
    // Arrange
    let key = KeyFile::Absent;

    // Act
    let problems = start_problems(key);

    // Assert
    assert_names_the_key(&problems);
}

#[test]
fn an_empty_key_file_is_refused_naming_the_key() {
    // Arrange
    let key = KeyFile::Bytes(b"");

    // Act
    let problems = start_problems(key);

    // Assert
    assert_names_the_key(&problems);
}

#[test]
fn a_key_file_holding_only_a_line_break_is_refused_naming_the_key() {
    // Arrange
    let key = KeyFile::Bytes(b"\n");

    // Act
    let problems = start_problems(key);

    // Assert
    assert_names_the_key(&problems);
}

#[test]
fn an_unreadable_key_file_is_refused_naming_the_key() {
    // Arrange
    let layouts = [KeyFile::Directory, KeyFile::DirectoryByVariable];

    // Act
    let outcomes: Vec<_> = layouts.into_iter().map(start_problems).collect();

    // Assert
    for problems in &outcomes {
        assert_names_the_key(problems);
    }
}

#[test]
fn a_31_byte_key_is_refused_naming_the_key() {
    // Arrange
    let short = &TEST_GRANT_KEY.as_bytes()[..GRANT_KEY_MIN_BYTES - 1];

    // Act
    let problems = start_problems(KeyFile::Bytes(short));

    // Assert
    assert_names_the_key(&problems);
    assert_eq!(problems[0], out_of_range(KEY, "32B", "no ceiling"));
}

#[test]
fn a_32_byte_key_is_accepted_with_or_without_a_trailing_line_break() {
    // Arrange
    let bare = TEST_GRANT_KEY.as_bytes().to_vec();
    let mut with_break = bare.clone();
    with_break.push(b'\n');

    // Act
    let outcomes = [bare, with_break].map(|bytes| start_problems(KeyFile::Bytes(&bytes)));

    // Assert
    assert_eq!(outcomes, [Vec::new(), Vec::new()]);
}

#[test]
fn a_key_counts_bytes_not_characters() {
    // Arrange
    let sixteen_two_byte_characters = "é".repeat(16);
    let fifteen = "é".repeat(15);

    // Act
    let accepted = start_problems(KeyFile::Bytes(sixteen_two_byte_characters.as_bytes()));
    let refused = start_problems(KeyFile::Bytes(fifteen.as_bytes()));

    // Assert
    assert_eq!(accepted, Vec::new());
    assert_names_the_key(&refused);
}

#[test]
fn an_empty_key_in_the_section_is_refused_naming_the_key() {
    // Arrange
    let mut config = valid();
    config.grant_key = Secret::new(String::new());

    // Act
    let problems = config.validate();

    // Assert
    assert_names_the_key(&problems);
    assert_eq!(problems[0], KeyProblem::new(KEY, Reason::MissingRequired));
}

#[test]
fn no_message_and_no_dump_contains_the_key() {
    // Arrange
    let secret = "s3cr3t-short-key-material";
    let (sources, loaded) = load_with_key(
        KeyFile::Bytes(secret.as_bytes()),
        Environment::Production,
        &[],
    );
    let settings = loaded.expect("a short key still loads");
    let config = ShareConfig::from_settings(&settings);
    let missing = ConfigError::MissingSecret(KEY.to_owned());

    // Act
    let texts = [
        config.validate()[0].to_string(),
        format!("{:?}", config.validate()),
        format!("{config:?}"),
        format!("{:?}", ShareConfig::load_refusal(&missing, &sources)),
    ];

    // Assert
    for text in texts {
        assert!(!text.contains(secret), "{text}");
    }
}

#[test]
fn load_refusal_ignores_an_error_about_something_else() {
    // Arrange
    let (sources, _) = load_with_key(KeyFile::Absent, Environment::Production, &[]);
    let other_secret = ConfigError::MissingSecret("worker_api_token".to_owned());
    let other_file =
        ConfigError::Unreadable(PathBuf::from("/run/secrets/other"), ErrorKind::NotFound);
    let other_key = ConfigError::Invalid(vec![KeyProblem::new("public_url", Reason::WrongType)]);

    // Act
    let refusals = [other_secret, other_file, other_key]
        .map(|error| ShareConfig::load_refusal(&error, &sources));

    // Assert
    assert_eq!(refusals, [None, None, None]);
}

#[test]
fn zero_values_that_would_expire_or_refuse_everything_are_refused() {
    // Arrange
    let cases: [(&str, Breaker); 7] = [
        ("share_default_ttl", |c| c.default_ttl = Duration::ZERO),
        ("share_max_ttl", |c| {
            c.default_ttl = Duration::ZERO;
            c.max_ttl = Duration::ZERO;
        }),
        ("share_grant_ttl", |c| c.grant_ttl = Duration::ZERO),
        ("share_wrong_password_window", |c| {
            c.wrong_password_window = Duration::ZERO;
        }),
        ("share_wrong_password_limit", |c| c.wrong_password_limit = 0),
        ("share_invalid_token_limit", |c| c.invalid_token_limit = 0),
        ("share_password_max_concurrency", |c| {
            c.password_max_concurrency = Some(0);
        }),
    ];

    // Act
    let refused: Vec<(&str, Vec<String>)> = cases
        .iter()
        .map(|(key, break_it)| {
            let mut config = valid();
            break_it(&mut config);
            let keys = config.validate().into_iter().map(|p| p.key).collect();
            (*key, keys)
        })
        .collect();

    // Assert
    for (key, keys) in refused {
        assert!(keys.iter().any(|named| named == key), "{key}: {keys:?}");
    }
}

#[test]
fn a_zero_lookup_cache_is_accepted() {
    // Arrange
    let mut config = valid();
    config.lookup_cache = Duration::ZERO;

    // Act
    let problems = config.validate();

    // Assert
    assert_eq!(problems, Vec::new());
}

#[test]
fn a_default_lifetime_above_the_maximum_is_refused_and_equal_is_accepted() {
    // Arrange
    let mut config = valid();
    config.max_ttl = Duration::from_secs(DAY);
    config.default_ttl = Duration::from_secs(DAY + 1);

    // Act
    let above = config.validate();
    config.default_ttl = Duration::from_secs(DAY);
    let equal = config.validate();

    // Assert
    assert_eq!(
        above,
        vec![out_of_range("share_default_ttl", "1ms", "share_max_ttl")]
    );
    assert_eq!(equal, Vec::new());
}

#[test]
fn unset_argon2_params_give_the_argon2_default_cost() {
    // Arrange
    let config = valid();

    // Act
    let params = config.password_params().expect("default cost");

    // Assert
    assert_eq!(
        (params.m_cost(), params.t_cost(), params.p_cost()),
        (19_456, 2, 1)
    );
}

#[test]
fn set_argon2_params_are_read_as_memory_passes_and_lanes() {
    // Arrange
    let config = config_from(&[("SUNNYCLOUD_SHARE_ARGON2_PARAMS", "65536, 3, 1")]);

    // Act
    let params = config.password_params().expect("set cost");

    // Assert
    assert_eq!(
        (params.m_cost(), params.t_cost(), params.p_cost()),
        (65_536, 3, 1)
    );
    assert_eq!(config.validate(), Vec::new());
}

#[test]
fn unusable_argon2_params_are_refused_naming_the_key() {
    // Arrange
    let values = [
        "19456,2",
        "a,b,c",
        "19456,2,1,4",
        "4,2,1",
        "19456,0,1",
        "19456,2,0",
    ];

    // Act
    let problems: Vec<Vec<KeyProblem>> = values
        .iter()
        .map(|value| {
            let mut config = valid();
            config.argon2_params = Some((*value).to_owned());
            config.validate()
        })
        .collect();

    // Assert
    for (value, found) in values.iter().zip(problems) {
        assert_eq!(
            found,
            vec![KeyProblem::new(ARGON2_PARAMS_KEY, Reason::WrongType)],
            "{value}"
        );
    }
}
