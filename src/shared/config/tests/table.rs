mod common;

use std::collections::BTreeSet;

use common::*;
use sc_config::{ConfigError, Environment, Reader, Reason, TABLE, ValueKind, parse_duration};

fn junk(kind: ValueKind) -> &'static str {
    match kind {
        ValueKind::Text => "not-a-choice",
        ValueKind::Flag => "maybe",
        ValueKind::Count => "12x",
        ValueKind::Size => "12XiB",
        ValueKind::Duration => "12x",
        ValueKind::Address => "not-an-address",
        ValueKind::List => "a,,b",
        ValueKind::Environment => "staging",
    }
}

fn is_checkable(spec: &sc_config::KeySpec) -> bool {
    !spec.secret && (spec.kind != ValueKind::Text || !spec.choices.is_empty())
}

fn readers_of(spec: &sc_config::KeySpec) -> impl Iterator<Item = Reader> + '_ {
    READERS.into_iter().filter(|r| spec.read_by.contains(*r))
}

#[test]
fn ac9_every_row_has_one_env_name_and_no_key_repeats() {
    let mut keys = BTreeSet::new();
    let mut env_names = BTreeSet::new();
    for spec in TABLE {
        assert!(keys.insert(spec.key), "duplicate key {}", spec.key);
        assert!(
            !spec.key.is_empty()
                && spec
                    .key
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "key {} is not flat snake case",
            spec.key
        );
        assert_eq!(
            spec.env_name(),
            format!("SUNNYCLOUD_{}", spec.key.to_uppercase())
        );
        assert!(
            env_names.insert(spec.env_name()),
            "duplicate env name {}",
            spec.env_name()
        );
    }
    assert_eq!(keys.len(), TABLE.len());
}

#[test]
fn ac9_no_env_name_collides_with_a_file_variable_or_the_tolerated_one() {
    let env_names: BTreeSet<String> = TABLE.iter().map(|s| s.env_name()).collect();
    for spec in TABLE.iter().filter(|s| s.secret) {
        assert!(!env_names.contains(&spec.file_env_name()), "{}", spec.key);
    }
    assert!(!env_names.contains("SUNNYCLOUD_VERSION"));
}

#[test]
fn compose_only_variables_are_not_keys() {
    for name in [
        "public_host",
        "blob_path",
        "worker_cpus",
        "worker_mem",
        "version",
        "url",
    ] {
        assert!(TABLE.iter().all(|s| s.key != name), "{name}");
    }
}

#[test]
fn every_row_has_a_reader_and_every_reader_has_rows() {
    for spec in TABLE {
        assert!(
            READERS.iter().any(|r| spec.read_by.contains(*r)),
            "{}",
            spec.key
        );
    }
    for reader in READERS {
        assert!(
            TABLE.iter().any(|s| s.read_by.contains(reader)),
            "{reader:?}"
        );
    }
}

#[test]
fn every_range_is_written_in_its_keys_own_syntax_with_min_not_above_max() {
    for spec in TABLE.iter().filter(|s| s.range.is_some()) {
        let (min, max) = spec.range.expect("range");
        let (lo, hi) = (spec.kind.parse_number(min), spec.kind.parse_number(max));
        assert!(
            lo.is_some() && hi.is_some(),
            "{} range {min}..{max}",
            spec.key
        );
        assert!(lo <= hi, "{} range {min}..{max}", spec.key);
        if spec.kind == ValueKind::Duration {
            assert!(spec.duration_range().is_some(), "{}", spec.key);
        }
    }
}

#[test]
fn every_default_parses_and_sits_inside_its_range() {
    for reader in READERS {
        let sources = valid_sources(reader);
        assert!(load_reader(reader, &sources).is_ok(), "{reader:?} defaults");
    }
    for spec in TABLE.iter().filter(|s| s.default.is_some()) {
        let default = spec.default.expect("default");
        if let Some((min, max)) = spec.range {
            let n = spec.kind.parse_number(default).expect(spec.key);
            assert!(
                spec.kind.parse_number(min).expect(spec.key) <= n,
                "{}",
                spec.key
            );
            assert!(
                n <= spec.kind.parse_number(max).expect(spec.key),
                "{}",
                spec.key
            );
        }
        assert!(!spec.optional, "{} is optional and has a default", spec.key);
    }
}

#[test]
fn every_key_accepts_its_default_written_explicitly() {
    for spec in TABLE.iter().filter(|s| s.default.is_some() && !s.secret) {
        for reader in readers_of(spec) {
            let mut sources = valid_sources(reader);
            set_key(&mut sources, spec.key, spec.default.expect("default"));
            assert!(
                load_reader(reader, &sources).is_ok(),
                "{} for {reader:?}",
                spec.key
            );
        }
    }
}

#[test]
fn every_key_rejects_malformed_text_naming_only_itself() {
    for spec in TABLE.iter().filter(|s| is_checkable(s)) {
        for reader in readers_of(spec) {
            let mut sources = valid_sources(reader);
            set_key(&mut sources, spec.key, junk(spec.kind));
            let problems = problems_of(load_reader(reader, &sources));
            assert_eq!(
                problems.len(),
                1,
                "{} for {reader:?}: {problems:?}",
                spec.key
            );
            assert_eq!(problems[0].key, spec.key);
            assert_eq!(problems[0].reason, Reason::WrongType, "{}", spec.key);
        }
    }
}

#[test]
fn every_key_is_ignored_by_binaries_that_do_not_read_it() {
    for spec in TABLE.iter().filter(|s| is_checkable(s)) {
        for reader in READERS.into_iter().filter(|r| !spec.read_by.contains(*r)) {
            let mut sources = valid_sources(reader);
            set_key(&mut sources, spec.key, junk(spec.kind));
            assert!(
                load_reader(reader, &sources).is_ok(),
                "{} must be ignored by {reader:?}",
                spec.key
            );
        }
    }
}

#[test]
fn every_required_key_is_reported_when_absent() {
    for spec in TABLE.iter().filter(|s| s.default.is_none() && !s.optional) {
        for reader in readers_of(spec) {
            let mut sources = valid_sources(reader);
            sources.env.remove(&spec.env_name());
            let private = TempDir::new();
            if spec.secret {
                for entry in std::fs::read_dir(&sources.secrets_dir).expect("secret dir") {
                    let name = entry.expect("entry").file_name();
                    if name.to_str() != Some(spec.key) {
                        std::fs::copy(sources.secrets_dir.join(&name), private.path().join(&name))
                            .expect("copy secret");
                    }
                }
                sources.secrets_dir = private.path().to_path_buf();
            }
            let result = load_reader(reader, &sources);
            if spec.secret {
                assert_eq!(
                    result,
                    Err(ConfigError::MissingSecret(spec.key.into())),
                    "{}",
                    spec.key
                );
            } else {
                assert_eq!(
                    problems_of(result),
                    vec![problem(spec.key, Reason::MissingRequired)],
                    "{}",
                    spec.key
                );
            }
        }
    }
}

fn above_and_below(spec: &sc_config::KeySpec) -> (String, Option<String>) {
    let (min, max) = spec.range.expect("range");
    let (lo, hi) = (
        spec.kind.parse_number(min).expect("min"),
        spec.kind.parse_number(max).expect("max"),
    );
    let unit = if spec.kind == ValueKind::Duration {
        "ms"
    } else {
        ""
    };
    let below = (lo > 0).then(|| format!("{}{unit}", lo - 1));
    (format!("{}{unit}", hi + 1), below)
}

#[test]
fn ranges_bind_in_production_and_durations_only_there() {
    for spec in TABLE.iter().filter(|s| s.range.is_some()) {
        let (above, below) = above_and_below(spec);
        let (min, max) = spec.range.expect("range");
        let is_duration = spec.kind == ValueKind::Duration;
        for reader in readers_of(spec) {
            let mut sources = valid_sources(reader);
            sources.environment = Environment::Production;
            set_key(&mut sources, spec.key, &above);
            assert_eq!(
                problems_of(load_reader(reader, &sources)),
                vec![problem(
                    spec.key,
                    Reason::OutOfRange {
                        min: min.into(),
                        max: max.into()
                    }
                )],
                "{} above",
                spec.key
            );
            if let Some(below) = &below {
                set_key(&mut sources, spec.key, below);
                let expected = if is_duration {
                    Reason::ForbiddenInRelease
                } else {
                    Reason::OutOfRange {
                        min: min.into(),
                        max: max.into(),
                    }
                };
                assert_eq!(
                    problems_of(load_reader(reader, &sources)),
                    vec![problem(spec.key, expected)],
                    "{} below",
                    spec.key
                );
            }
            for environment in [Environment::Test, Environment::Development] {
                sources.environment = environment;
                for value in std::iter::once(&above).chain(below.iter()) {
                    set_key(&mut sources, spec.key, value);
                    assert_eq!(
                        load_reader(reader, &sources).is_ok(),
                        is_duration,
                        "{} = {value} in {environment:?}",
                        spec.key
                    );
                }
            }
        }
    }
}

#[test]
fn duration_ranges_cover_the_helper_table_floors_and_ceilings() {
    let expected = [
        ("upload_session_ttl", "1m", "7d"),
        ("orphan_sweep_age", "1m", "30d"),
        ("refresh_reuse_grace", "1s", "60s"),
        ("share_default_ttl", "1m", "365d"),
        ("share_max_ttl", "1m", "3650d"),
        ("provider_timeout", "1s", "30s"),
        ("tombstone_horizon", "90d", "365d"),
        ("longpoll_wait_max", "1s", "55s"),
        ("account_delete_deadline", "1m", "60m"),
        ("retention", "0s", "90d"),
        ("job_lease", "5s", "1h"),
        ("desktop_settle", "200ms", "30s"),
        ("desktop_rescan_interval", "10s", "24h"),
    ];
    for (key, min, max) in expected {
        let spec = TABLE.iter().find(|s| s.key == key).expect(key);
        assert_eq!(spec.range, Some((min, max)), "{key}");
        let (floor, ceiling) = spec.duration_range().expect(key);
        assert_eq!(Some(floor), parse_duration(min));
        assert_eq!(Some(ceiling), parse_duration(max));
    }
}
