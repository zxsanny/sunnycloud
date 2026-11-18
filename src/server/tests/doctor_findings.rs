//! AZ-3809 and AZ-4022 against the real binary: configuration problems, a refused database role and the healthy
//! stack are findings of `doctor`, never a crash and never a secret.

mod root_support;

use std::process::{Command, Stdio};

use root_support::{Root, Setup};
use serde_json::Value;

const PLANTED: &str = "planted-db-password-7f3a9c";

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn doctor(setup: &Setup, args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_sunnycloud"))
        .arg("--json")
        .arg("doctor")
        .args(args)
        .env_clear()
        .envs(&setup.env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("the binary runs");
    Run {
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).expect("UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("UTF-8"),
    }
}

/// `(check, level, message)` of every finding in the `--json` envelope.
fn findings(run: &Run) -> Vec<(String, String, String)> {
    let envelope: Value = serde_json::from_str(&run.stdout)
        .unwrap_or_else(|error| panic!("one JSON object: {error}: {}\n{}", run.stdout, run.stderr));
    envelope["data"]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .map(|item| {
            let text = |key: &str| item[key].as_str().expect("text").to_owned();
            (text("check"), text("level"), text("message"))
        })
        .collect()
}

fn of<'a>(all: &'a [(String, String, String)], check: &str) -> Vec<&'a (String, String, String)> {
    all.iter().filter(|item| item.0 == check).collect()
}

/// A provider that cannot be reached, so no test depends on the internet.
fn local_oidc(setup: &mut Setup) {
    setup
        .remove("github_client_id")
        .without_secret("provider_github_client_secret");
    setup
        .set("oidc_issuer", "http://127.0.0.1:1")
        .set("oidc_client_id", "client")
        .set("allow_plain_http_dev", "true")
        .secret("provider_oidc_client_secret", "oidc-secret");
}

#[test]
fn configuration_problems_are_one_fail_per_key_and_never_print_a_secret() {
    // Arrange
    let mut setup = Setup::valid(0);
    setup.set("retention", "91d");
    setup.set("bogus", "1");

    // Act
    let run = doctor(&setup, &["--pre-flight"]);

    // Assert
    assert_eq!(run.code, Some(2), "{}", run.stderr);
    let all = findings(&run);
    let config = of(&all, "configuration");
    assert_eq!(config.len(), 2, "{all:?}");
    assert!(config.iter().all(|item| item.1 == "fail"));
    assert!(config.iter().any(|item| item.2.contains("retention")));
    assert!(
        config
            .iter()
            .any(|item| item.2.contains("SUNNYCLOUD_BOGUS"))
    );
    assert!(!run.stdout.contains(PLANTED) && !run.stderr.contains(PLANTED));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_database_role_that_owns_the_schema_is_a_finding_and_the_stack_failure_follows() {
    // Arrange
    let mut root = Root::new().await;
    let owner = root
        .db
        .config()
        .db_migrator_url
        .expect("migrator URL")
        .expose()
        .clone();
    root.setup.secret("db_api_url", &owner);

    // Act
    let run = doctor(&root.setup, &[]);

    // Assert
    assert_eq!(run.code, Some(2), "{}", run.stderr);
    let all = findings(&run);
    let database = of(&all, "database");
    assert!(
        database.iter().any(|item| item.1 == "fail"),
        "the role problem is a Fail: {all:?}"
    );
    assert_eq!(of(&all, "stack").len(), 1, "{all:?}");
    assert!(!run.stdout.contains(&owner));
    root.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_healthy_stack_gives_ok_database_findings_and_the_provider_and_setup_findings() {
    // Arrange
    let mut root = Root::new().await;
    local_oidc(&mut root.setup);

    // Act
    let run = doctor(&root.setup, &[]);

    // Assert
    let all = findings(&run);
    let database = of(&all, "database");
    assert_eq!(database.len(), 4, "{all:?}");
    assert!(database.iter().all(|item| item.1 == "ok"), "{database:?}");
    let providers = of(&all, "identity_providers");
    assert_eq!(providers.len(), 1);
    assert_eq!(
        providers[0].1, "warn",
        "an unreachable provider is a warning"
    );
    assert!(
        providers[0]
            .2
            .contains("https://cloud.example.org/api/v1/auth/providers/oidc/callback")
    );
    let setup = of(&all, "identity_setup");
    assert_eq!(
        (setup[0].1.as_str(), setup[0].2.contains("setup pending")),
        ("ok", true)
    );
    assert_eq!(of(&all, "quota_drift")[0].1, "ok");
    assert_eq!(of(&all, "blob_files_missing")[0].1, "ok");
    root.finish().await;
}

const FIRST: &str = "00000000-0000-0000-0000-0000000000a1";
const DRIFTED: &str = "00000000-0000-0000-0000-0000000000a2";
const DELETING: &str = "00000000-0000-0000-0000-0000000000a3";

async fn account(root: &Root, id: &str, state: &str, used_bytes: i64) {
    root.db
        .admin_exec(&format!(
            "INSERT INTO accounts (id, state, created_at, display_name, role) \
             VALUES ('{id}', '{state}', 'epoch'::timestamptz, 'Fixture', 'user'); \
             INSERT INTO quota (account_id, used_bytes) VALUES ('{id}', {used_bytes})"
        ))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn quota_drift_names_the_one_drifted_account_and_changes_no_row() {
    // Arrange
    let mut root = Root::new().await;
    local_oidc(&mut root.setup);
    account(&root, FIRST, "active", 0).await;
    account(&root, DRIFTED, "active", 1000).await;
    account(&root, DELETING, "deleting", 5000).await;
    let before = root
        .db
        .admin_count("SELECT sum(used_bytes + reserved_bytes)::bigint FROM quota")
        .await;

    // Act
    let run = doctor(&root.setup, &[]);

    // Assert
    let all = findings(&run);
    let drift = of(&all, "quota_drift");
    assert_eq!(drift.len(), 1, "{all:?}");
    assert_eq!(drift[0].1, "warn");
    assert!(drift[0].2.contains(DRIFTED), "{}", drift[0].2);
    assert!(
        drift[0].2.contains("1000") && drift[0].2.contains(" 0 bytes"),
        "{}",
        drift[0].2
    );
    let after = root
        .db
        .admin_count("SELECT sum(used_bytes + reserved_bytes)::bigint FROM quota")
        .await;
    assert_eq!(after, before);
    root.finish().await;
}

const ACCOUNT: &str = "00000000-0000-0000-0000-0000000000b1";

/// An account with a folder `docs` and three files whose blobs are committed but have no file on the volume:
/// `docs/one.txt`, `docs/two.txt` and `three.txt`. Revision `n` is `…c<n>`, its blob `…d<n>`.
async fn three_files_without_blobs(root: &Root) {
    let mut sql = format!(
        "INSERT INTO accounts (id, state, created_at, display_name, role) \
         VALUES ('{ACCOUNT}', 'active', 'epoch'::timestamptz, 'Fixture', 'user'); \
         INSERT INTO quota (account_id) VALUES ('{ACCOUNT}'); \
         INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, created_at, updated_at) \
         VALUES ('00000000-0000-0000-0000-0000000000e0', '{ACCOUNT}', NULL, 'folder', '', '', 1, 'epoch', 'epoch'), \
         ('00000000-0000-0000-0000-0000000000e1', '{ACCOUNT}', '00000000-0000-0000-0000-0000000000e0', 'folder', 'docs', 'docs', 1, 'epoch', 'epoch'); "
    );
    for (n, parent, name) in [
        (1, "e1", "one.txt"),
        (2, "e1", "two.txt"),
        (3, "e0", "three.txt"),
    ] {
        sql.push_str(&format!(
            "INSERT INTO blobs (id, account_id, state, size, wrapped_key, master_key_id, created_at) \
             VALUES ('00000000-0000-0000-0000-0000000000d{n}', '{ACCOUNT}', 'committed', 1, '\\x00', 1, 'epoch'); \
             INSERT INTO nodes (id, account_id, parent_id, kind, name, name_key, name_key_version, current_revision_id, created_at, updated_at) \
             VALUES ('00000000-0000-0000-0000-00000000f00{n}', '{ACCOUNT}', '00000000-0000-0000-0000-0000000000{parent}', 'file', '{name}', '{name}', 1, '00000000-0000-0000-0000-0000000000c{n}', 'epoch', 'epoch'); \
             INSERT INTO revisions (id, account_id, node_id, blob_id, size, content_hash, blocklist, created_at) \
             VALUES ('00000000-0000-0000-0000-0000000000c{n}', '{ACCOUNT}', '00000000-0000-0000-0000-00000000f00{n}', \
             '00000000-0000-0000-0000-0000000000d{n}', 1, decode(repeat('00', 32), 'hex'), '\\x', 'epoch'); "
        ));
    }
    root.db.admin_exec(&sql).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn each_revision_whose_blob_file_is_gone_is_a_fail_with_path_account_and_revision() {
    // Arrange
    let mut root = Root::new().await;
    local_oidc(&mut root.setup);
    three_files_without_blobs(&root).await;

    // Act
    let run = doctor(&root.setup, &[]);

    // Assert
    assert_eq!(run.code, Some(2), "{}", run.stderr);
    let all = findings(&run);
    let missing = of(&all, "blob_files_missing");
    assert_eq!(missing.len(), 3, "{all:?}");
    for (n, path) in [(1, "docs/one.txt"), (2, "docs/two.txt"), (3, "three.txt")] {
        let revision = format!("00000000-0000-0000-0000-0000000000c{n}");
        let line = missing
            .iter()
            .find(|item| item.2.contains(&revision))
            .unwrap_or_else(|| panic!("revision {n} is listed: {missing:?}"));
        assert_eq!(line.1, "fail");
        assert!(
            line.2.contains(ACCOUNT) && line.2.contains(path),
            "{}",
            line.2
        );
    }
    root.finish().await;
}

#[test]
fn an_oidc_issuer_that_is_no_url_is_one_configuration_finding_not_an_abort() {
    // Arrange: `sc_config` only checks that the issuer is present; 01's provider build refuses the value.
    let mut setup = Setup::valid(0);
    local_oidc(&mut setup);
    setup.set("oidc_issuer", "not a url");

    // Act
    let run = doctor(&setup, &["--pre-flight"]);

    // Assert
    assert_eq!(run.code, Some(2), "{}", run.stderr);
    let all = findings(&run);
    let config = of(&all, "configuration");
    assert_eq!(config.len(), 1, "{all:?}");
    assert_eq!(config[0].1, "fail");
    assert!(config[0].2.contains("issuer"), "{all:?}");
}
