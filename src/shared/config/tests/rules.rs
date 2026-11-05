mod common;

use common::*;
use sc_config::{
    ApiSettings, ConfigError, ConfigSources, Environment, KeyProblem, Reader, Reason,
    WorkerSettings, load,
};

fn api_sources() -> ConfigSources {
    valid_sources(Reader::Api)
}

#[test]
fn ac1_retention_over_ceiling_refuses_start_in_production() {
    let mut sources = api_sources();
    set_key(&mut sources, "environment", "production");
    set_key(&mut sources, "retention", "91d");

    let result = load::<ApiSettings>(&sources);

    let Err(ConfigError::Invalid(problems)) = result else {
        panic!("expected Invalid, got {result:?}");
    };
    assert_eq!(
        problems,
        vec![problem(
            "retention",
            Reason::OutOfRange {
                min: "0s".into(),
                max: "90d".into()
            }
        )]
    );
}

#[test]
fn ac1_error_text_names_the_key_and_the_range() {
    let mut sources = api_sources();
    set_key(&mut sources, "retention", "91d");

    let text = load::<ApiSettings>(&sources).unwrap_err().to_string();

    assert!(text.contains("retention"), "{text}");
    assert!(text.contains("0s to 90d"), "{text}");
}

#[test]
fn ac1_retention_at_the_edges_is_accepted() {
    for value in ["0", "7d", "90d"] {
        let mut sources = api_sources();
        set_key(&mut sources, "retention", value);
        assert!(load::<ApiSettings>(&sources).is_ok(), "{value}");
    }
}

#[test]
fn ac1_ceiling_does_not_bind_outside_production() {
    for environment in [Environment::Test, Environment::Development] {
        let mut sources = api_sources();
        sources.environment = environment;
        set_key(&mut sources, "retention", "91d");
        assert!(load::<ApiSettings>(&sources).is_ok(), "{environment:?}");
    }
}

#[test]
fn ac2_five_invalid_keys_give_one_error_with_five_problems() {
    let dir = TempDir::new();
    let file = dir.write(
        "config.json",
        r#"{
            "retention": "91d",
            "shutdown_grace": "soon",
            "max_connections": "many",
            "log": "yaml",
            "h2_stream_window": "huge"
        }"#,
    );
    let mut sources = api_sources();
    sources.files.push(file);

    let problems = problems_of(load_reader(Reader::Api, &sources));

    let keys: Vec<&str> = problems.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(problems.len(), 5, "{keys:?}");
    for key in [
        "retention",
        "shutdown_grace",
        "max_connections",
        "log",
        "h2_stream_window",
    ] {
        assert!(keys.contains(&key), "{key} missing in {keys:?}");
    }
}

#[test]
fn ac3_unknown_variables_are_all_named_and_version_is_tolerated() {
    let mut sources = api_sources();
    set(&mut sources, "SUNNYCLOUD_TYPO", "1");
    set(&mut sources, "SUNNYCLOUD_ANOTHER_TYPO", "1");
    set(&mut sources, "SUNNYCLOUD_VERSION", "1.0.0");
    set(&mut sources, "PUBLIC_HOST", "compose-only.example");

    let problems = problems_of(load_reader(Reader::Api, &sources));

    assert_eq!(
        problems,
        vec![
            problem("SUNNYCLOUD_ANOTHER_TYPO", Reason::Unknown),
            problem("SUNNYCLOUD_TYPO", Reason::Unknown),
        ]
    );
}

#[test]
fn ac3_names_are_exact_upper_case_and_files_exist_only_for_secrets() {
    let mut sources = api_sources();
    set(&mut sources, "SUNNYCLOUD_retention", "7d");
    set(&mut sources, "SUNNYCLOUD_RETENTION_FILE", "/nowhere");

    let problems = problems_of(load_reader(Reader::Api, &sources));

    let keys: Vec<&str> = problems.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(
        keys,
        vec!["SUNNYCLOUD_RETENTION_FILE", "SUNNYCLOUD_retention"]
    );
}

#[test]
fn ac3_unknown_key_in_the_config_file_is_refused() {
    let dir = TempDir::new();
    let file = dir.write(
        "config.json",
        r#"{"retention_days": 7, "desktop.wake_gap": "5s"}"#,
    );
    let mut sources = api_sources();
    sources.files.push(file);

    let problems = problems_of(load_reader(Reader::Api, &sources));

    assert_eq!(
        problems,
        vec![
            problem("desktop.wake_gap", Reason::Unknown),
            problem("retention_days", Reason::Unknown)
        ]
    );
}

#[test]
fn ac5_below_floor_is_accepted_in_test_and_forbidden_in_production() {
    let mut sources = api_sources();
    set_key(&mut sources, "upload_session_ttl", "10s");

    sources.environment = Environment::Test;
    let in_test = load::<ApiSettings>(&sources).expect("test load");
    sources.environment = Environment::Production;
    let in_production = problems_of(load_reader(Reader::Api, &sources));

    assert_eq!(in_test.upload_session_ttl.get().as_secs(), 10);
    assert_eq!(
        in_production,
        vec![problem("upload_session_ttl", Reason::ForbiddenInRelease)]
    );
}

#[test]
fn environment_variable_overrides_the_sources_environment() {
    let mut sources = api_sources();
    set_key(&mut sources, "upload_session_ttl", "10s");
    set_key(&mut sources, "environment", "test");

    let section = load::<ApiSettings>(&sources).expect("test load");

    assert_eq!(section.environment, Environment::Test);
}

#[test]
fn malformed_environment_value_is_reported() {
    let mut sources = api_sources();
    set_key(&mut sources, "environment", "staging");

    let problems = problems_of(load_reader(Reader::Api, &sources));

    assert_eq!(problems, vec![problem("environment", Reason::WrongType)]);
}

#[test]
fn ac8_no_sign_in_provider_names_the_provider_settings() {
    let mut sources = api_sources();
    let _dir = without_github_provider(&mut sources);

    let problems = problems_of(load_reader(Reader::Api, &sources));

    assert_eq!(
        problems,
        vec![
            problem("github_client_id", Reason::MissingRequired),
            problem("oidc_issuer", Reason::MissingRequired),
        ]
    );
}

#[test]
fn ac8_half_configured_provider_names_the_missing_member() {
    let mut sources = api_sources();
    let _dir = without_github_provider(&mut sources);
    set_key(&mut sources, "oidc_issuer", "https://idp.example");
    set_key(&mut sources, "oidc_client_id", "client");

    let problems = problems_of(load_reader(Reader::Api, &sources));

    assert_eq!(
        problems,
        vec![problem(
            "provider_oidc_client_secret",
            Reason::MissingRequired
        )]
    );
}

#[test]
fn ac8_oidc_alone_satisfies_the_rule() {
    let mut sources = api_sources();
    let dir = without_github_provider(&mut sources);
    set_key(&mut sources, "oidc_issuer", "https://idp.example");
    set_key(&mut sources, "oidc_client_id", "client");
    dir.write("provider_oidc_client_secret", "secret");

    assert!(load::<ApiSettings>(&sources).is_ok());
}

#[test]
fn public_url_must_be_https_outside_development() {
    let mut sources = api_sources();
    set_key(&mut sources, "public_url", "http://cloud.example");

    for (environment, expected) in [
        (Environment::Production, Some(Reason::ForbiddenInRelease)),
        (Environment::Test, Some(Reason::ForbiddenInRelease)),
        (Environment::Development, None),
    ] {
        sources.environment = environment;
        let result = load_reader(Reader::Api, &sources);
        match expected {
            Some(reason) => {
                assert_eq!(problems_of(result), vec![problem("public_url", reason)])
            }
            None => assert!(result.is_ok(), "{environment:?}"),
        }
    }
}

#[test]
fn public_url_without_a_scheme_is_wrong_type_and_missing_is_required() {
    let mut sources = api_sources();
    set_key(&mut sources, "public_url", "cloud.example");
    assert_eq!(
        problems_of(load_reader(Reader::Api, &sources)),
        vec![problem("public_url", Reason::WrongType)]
    );

    sources.env.remove("SUNNYCLOUD_PUBLIC_URL");
    assert_eq!(
        problems_of(load_reader(Reader::Api, &sources)),
        vec![problem("public_url", Reason::MissingRequired)]
    );
}

#[test]
fn job_attempts_range_binds_in_every_environment() {
    let mut sources = api_sources();
    sources.environment = Environment::Test;
    set_key(&mut sources, "job_attempts", "11");

    let problems = problems_of(load_reader(Reader::Api, &sources));

    assert_eq!(
        problems,
        vec![problem(
            "job_attempts",
            Reason::OutOfRange {
                min: "1".into(),
                max: "10".into()
            }
        )]
    );
}

#[test]
fn later_sources_win_file_then_environment() {
    let dir = TempDir::new();
    let first = dir.write(
        "a.json",
        r#"{"max_connections": 10, "gc_batch": "20", "log": "json"}"#,
    );
    let second = dir.write("b.json", r#"{"max_connections": 11}"#);
    let mut sources = api_sources();
    sources.files = vec![first, second];
    set_key(&mut sources, "gc_batch", "30");

    let section = load::<ApiSettings>(&sources).expect("load");

    assert_eq!(section.max_connections, 11);
    assert_eq!(section.gc_batch, 30);
    assert_eq!(section.log, "json");
    assert_eq!(section.shutdown_grace.get().as_secs(), 25);
}

#[test]
fn file_values_accept_json_numbers_booleans_and_arrays() {
    let dir = TempDir::new();
    let file = dir.write(
        "config.json",
        r#"{"db_pool_max": 7, "blob_preallocate": false, "trusted_proxies": ["10.0.0.1", "10.0.0.2"]}"#,
    );
    let mut sources = api_sources();
    sources.files.push(file);

    let section = load::<ApiSettings>(&sources).expect("load");

    assert_eq!(section.db_pool_max, 7);
    assert!(!section.blob_preallocate);
    assert_eq!(section.trusted_proxies, vec!["10.0.0.1", "10.0.0.2"]);
}

#[test]
fn malformed_config_documents_are_problems_not_panics() {
    let dir = TempDir::new();
    for (name, text) in [
        ("a.json", "{not json"),
        ("b.json", "[1, 2]"),
        ("c.json", "null"),
    ] {
        let mut sources = api_sources();
        sources.files.push(dir.write(name, text));
        let problems = problems_of(load_reader(Reader::Api, &sources));
        assert_eq!(problems.len(), 1, "{name}");
        assert_eq!(problems[0].reason, Reason::WrongType);
    }
}

#[test]
fn nested_or_null_values_in_the_file_are_wrong_type() {
    let dir = TempDir::new();
    let file = dir.write("config.json", r#"{"retention": null, "log": {"x": 1}}"#);
    let mut sources = api_sources();
    sources.files.push(file);

    let problems = problems_of(load_reader(Reader::Api, &sources));

    assert_eq!(
        problems,
        vec![
            problem("log", Reason::WrongType),
            problem("retention", Reason::WrongType)
        ]
    );
}

#[test]
fn missing_config_file_is_unreadable() {
    let mut sources = api_sources();
    sources.files.push("/nowhere/config.json".into());

    let error = load::<ApiSettings>(&sources).unwrap_err();

    assert_eq!(
        error,
        ConfigError::Unreadable("/nowhere/config.json".into(), std::io::ErrorKind::NotFound)
    );
}

#[test]
fn a_binary_ignores_keys_it_does_not_read() {
    let mut sources = valid_sources(Reader::Worker);
    set_key(&mut sources, "retention", "91d");
    set_key(&mut sources, "log", "yaml");

    let section = load::<WorkerSettings>(&sources).expect("worker load");

    assert_eq!(section.worker_api_url, "http://api:8081");
}

#[test]
fn defaults_match_the_key_table() {
    let section = load::<ApiSettings>(&api_sources()).expect("load");

    assert_eq!(section.environment, Environment::Production);
    assert_eq!(section.internal_addr.get().to_string(), "0.0.0.0:8081");
    assert_eq!(section.h2_stream_window.bytes(), 4 << 20);
    assert_eq!(section.upload_session_ttl.get().as_secs(), 24 * 3600);
    assert_eq!(section.retention.get().as_secs(), 7 * 86_400);
    assert_eq!(section.blob_sync_coalesce_delay.get().as_millis(), 0);
    assert!(section.blob_preallocate);
    assert!(section.cli_confirm_required);
    assert!(!section.allow_plain_http_dev);
    assert_eq!(section.default_quota.bytes(), 100 << 30);
    assert_eq!(section.job_attempts, 3);
    assert!(section.trusted_proxies.is_empty());
    assert!(section.blob_io_threads.is_none());
}

#[test]
fn several_problem_kinds_come_back_together() {
    let mut sources = api_sources();
    set(&mut sources, "SUNNYCLOUD_TYPO", "1");
    set_key(&mut sources, "retention", "91d");
    set_key(&mut sources, "max_connections", "x");
    sources.env.remove("SUNNYCLOUD_PUBLIC_URL");

    let problems: Vec<KeyProblem> = problems_of(load_reader(Reader::Api, &sources));

    let mut keys: Vec<&str> = problems.iter().map(|p| p.key.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "SUNNYCLOUD_TYPO",
            "max_connections",
            "public_url",
            "retention"
        ]
    );
}
