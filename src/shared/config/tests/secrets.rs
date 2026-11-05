mod common;

use std::collections::BTreeMap;

use common::*;
use sc_config::{ApiSettings, ConfigError, ConfigSources, Reader, Reason, Secret, load};

const PLANTED: &str = "planted-S3CR3T-0f2a9c";

fn sources_with_secret_dir(dir: &TempDir) -> ConfigSources {
    let mut sources = valid_sources(Reader::Api);
    for name in [
        "SUNNYCLOUD_DB_API_URL",
        "SUNNYCLOUD_SHARE_GRANT_KEY",
        "SUNNYCLOUD_MASTER_KEY_FILE",
        "SUNNYCLOUD_WORKER_API_TOKEN",
    ] {
        sources.env.remove(name);
    }
    dir.write("provider_github_client_secret", "gh-secret");
    sources.secrets_dir = dir.path().to_path_buf();
    sources
}

#[test]
fn ac4_planted_secret_reaches_no_output() {
    let dir = TempDir::new();
    let planted = dir.write("planted", &format!("{PLANTED}\n"));
    let mut sources = sources_with_secret_dir(&dir);
    set(
        &mut sources,
        "SUNNYCLOUD_DB_API_URL_FILE",
        planted.to_str().unwrap(),
    );
    dir.write("share_grant_key", PLANTED);
    for key in ["master_key_file", "worker_api_token"] {
        dir.write(key, "other-secret\n");
    }

    let section = load::<ApiSettings>(&sources).expect("load");
    let outputs = [
        format!("{section:?}"),
        format!("{:#?}", *section),
        format!("{}", section.db_api_url),
        format!("{:?}", section.share_grant_key),
        format!("{sources:?}"),
    ];

    assert_eq!(section.db_api_url.expose(), PLANTED);
    for output in &outputs {
        assert!(!output.contains(PLANTED), "leaked in {output}");
    }
    assert!(outputs[0].contains("db_api_url: ***"));
}

#[test]
fn ac4_missing_second_secret_is_missing_secret_naming_its_key() {
    let dir = TempDir::new();
    dir.write("db_api_url", PLANTED);
    dir.write("master_key_file", "k");
    dir.write("worker_api_token", "t");
    let sources = sources_with_secret_dir(&dir);

    let error = load::<ApiSettings>(&sources).unwrap_err();

    assert_eq!(error, ConfigError::MissingSecret("share_grant_key".into()));
    assert!(!format!("{error} {error:?}").contains(PLANTED));
}

#[test]
fn missing_secret_beside_other_problems_is_listed_in_the_invalid_error() {
    let dir = TempDir::new();
    let mut sources = sources_with_secret_dir(&dir);
    set_key(&mut sources, "max_connections", "x");

    let problems = problems_of(load_reader(Reader::Api, &sources));

    let keys: Vec<(&str, &Reason)> = problems
        .iter()
        .map(|p| (p.key.as_str(), &p.reason))
        .collect();
    assert_eq!(keys.len(), 5, "{keys:?}");
    assert!(keys.contains(&("max_connections", &Reason::WrongType)));
    for key in [
        "db_api_url",
        "share_grant_key",
        "master_key_file",
        "worker_api_token",
    ] {
        assert!(keys.contains(&(key, &Reason::MissingRequired)), "{key}");
    }
}

#[test]
fn default_secret_path_and_file_variable_both_read() {
    let dir = TempDir::new();
    dir.write("db_api_url", "from-default-path\r\n");
    let other = dir.write("elsewhere", "from-file-variable");
    dir.write("share_grant_key", "g");
    dir.write("master_key_file", "k");
    dir.write("worker_api_token", "t");
    let mut sources = sources_with_secret_dir(&dir);

    let by_default = load::<ApiSettings>(&sources).expect("default path");
    set(
        &mut sources,
        "SUNNYCLOUD_DB_API_URL_FILE",
        other.to_str().unwrap(),
    );
    let by_variable = load::<ApiSettings>(&sources).expect("file variable");

    assert_eq!(by_default.db_api_url.expose(), "from-default-path");
    assert_eq!(by_variable.db_api_url.expose(), "from-file-variable");
}

#[test]
fn secret_file_wins_over_the_plain_variable() {
    let dir = TempDir::new();
    dir.write("db_api_url", "from-file");
    let mut sources = sources_with_secret_dir(&dir);
    for key in ["share_grant_key", "master_key_file", "worker_api_token"] {
        dir.write(key, "x");
    }
    set_key(&mut sources, "db_api_url", "from-variable");

    let section = load::<ApiSettings>(&sources).expect("load");

    assert_eq!(section.db_api_url.expose(), "from-file");
}

#[test]
fn unreadable_secret_file_is_unreadable_not_missing() {
    let dir = TempDir::new();
    let mut sources = sources_with_secret_dir(&dir);
    set(&mut sources, "SUNNYCLOUD_DB_API_URL_FILE", "/nowhere/db");

    let error = load::<ApiSettings>(&sources).unwrap_err();

    assert_eq!(
        error,
        ConfigError::Unreadable("/nowhere/db".into(), std::io::ErrorKind::NotFound)
    );
}

#[test]
fn empty_secret_file_counts_as_absent() {
    let dir = TempDir::new();
    dir.write("db_api_url", "\n");
    for key in ["share_grant_key", "master_key_file", "worker_api_token"] {
        dir.write(key, "x");
    }
    let sources = sources_with_secret_dir(&dir);

    assert_eq!(
        load::<ApiSettings>(&sources).unwrap_err(),
        ConfigError::MissingSecret("db_api_url".into())
    );
}

#[test]
fn secret_file_variable_of_a_plain_key_is_unknown() {
    let mut sources = valid_sources(Reader::Api);
    set(&mut sources, "SUNNYCLOUD_PUBLIC_URL_FILE", "/x");

    let problems = problems_of(load_reader(Reader::Api, &sources));

    assert_eq!(
        problems,
        vec![problem("SUNNYCLOUD_PUBLIC_URL_FILE", Reason::Unknown)]
    );
}

#[test]
fn every_secret_row_prints_as_stars_and_secret_rows_are_marked() {
    let secrets: Vec<&str> = sc_config::TABLE
        .iter()
        .filter(|s| s.secret)
        .map(|s| s.key)
        .collect();
    assert!(secrets.contains(&"db_api_url"));
    assert!(secrets.contains(&"share_grant_key"));
    assert_eq!(Secret::new(1_u8).to_string(), "***");
    let mut sources = ConfigSources {
        env: BTreeMap::new(),
        ..ConfigSources::default()
    };
    for key in &secrets {
        set_key(&mut sources, key, PLANTED);
    }
    assert!(!format!("{sources:?}").contains(PLANTED));
}

#[test]
fn secret_from_a_plain_variable_is_not_accepted() {
    let dir = TempDir::new();
    let mut sources = sources_with_secret_dir(&dir);
    for key in ["share_grant_key", "master_key_file", "worker_api_token"] {
        dir.write(key, "x");
    }
    set_key(&mut sources, "db_api_url", "from-variable");

    let error = load::<ApiSettings>(&sources).expect_err("a secret needs a file");

    assert_eq!(error, ConfigError::MissingSecret("db_api_url".into()));
}
