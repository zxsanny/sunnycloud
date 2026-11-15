//! AZ-3791 AC-1 to AC-4 against the real `sunnycloud serve` binary on PostgreSQL 18 and a real blob volume: the
//! start steps in their documented order and `/readyz` 200 within 10 s, and each refusal (newer or pending
//! schema, master key problems, the schema owner's URL) exits non-zero before any listener accepts.

mod root_support;
mod serve_support;

use std::fs;
use std::time::Duration;

use serve_support::{Refused, START_STEPS, Stack, json_lines, positions};
use sqlx::Connection;
use uuid::Uuid;

const WITHIN: Duration = Duration::from_secs(10);
const FUTURE: i64 = 99_990_101_000_000;

fn assert_refused(refused: &Refused, names: &[&str]) {
    let stderr = refused.exited.stderr_text();
    assert_eq!(refused.exited.code, Some(2), "{stderr}");
    assert!(
        refused.exited.elapsed < WITHIN,
        "{:?}",
        refused.exited.elapsed
    );
    assert!(
        !refused.accepted,
        "a listener accepted before the refusal:\n{stderr}"
    );
    for name in names {
        assert!(stderr.contains(name), "`{name}` missing from:\n{stderr}");
    }
    let messages = refused.exited.messages();
    assert!(
        !messages
            .iter()
            .any(|message| message == "serve start: listeners"),
        "{messages:#?}"
    );
}

/// The `_sqlx_migrations` rows as text, to show `serve` changed none.
async fn migrations(stack: &Stack) -> String {
    let mut admin = stack.root.db.admin().await;
    let rows: String = sqlx::query_scalar(
        "SELECT string_agg(version::text || ':' || success::text || ':' || encode(checksum, 'hex'), ',' \
         ORDER BY version) FROM _sqlx_migrations",
    )
    .fetch_one(&mut admin)
    .await
    .expect("migration rows");
    admin.close().await.expect("close admin connection");
    rows
}

#[tokio::test(flavor = "multi_thread")]
async fn start_steps_are_logged_in_order_and_readyz_is_200_within_10s() {
    // Arrange
    let stack = Stack::new().await;
    let mut server = stack.spawn();

    // Act
    let ready_after = server.wait_ready(stack.ports.ops, WITHIN);
    server.signal("TERM");
    let exited = server.wait_exit(Duration::from_secs(10));

    // Assert
    assert!(ready_after < WITHIN, "{ready_after:?}");
    assert_eq!(exited.code, Some(0), "{}", exited.stderr_text());
    let messages = exited.messages();
    let at = positions(&messages, &START_STEPS);
    assert!(
        at.windows(2).all(|pair| pair[0] < pair[1]),
        "out of order: {messages:#?}"
    );
    let stamps: Vec<String> = json_lines(&exited.stderr)
        .iter()
        .filter(|line| {
            line["msg"]
                .as_str()
                .is_some_and(|msg| START_STEPS.contains(&msg))
        })
        .map(|line| line["ts"].as_str().expect("ts").to_owned())
        .collect();
    assert_eq!(stamps.len(), START_STEPS.len());
    assert!(
        stamps.windows(2).all(|pair| pair[0] <= pair[1]),
        "{stamps:?}"
    );
    stack.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_database_newer_than_the_binary_refuses_and_names_migrate() {
    // Arrange
    let stack = Stack::new().await;
    stack
        .root
        .db
        .admin_exec(&format!(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
             VALUES ({FUTURE}, 'future', true, '\\x00'::bytea, 0)"
        ))
        .await;
    let before = migrations(&stack).await;

    // Act
    let refused = stack.refused();

    // Assert
    assert_refused(&refused, &["UnknownNewer", "sunnycloud migrate"]);
    assert_eq!(migrations(&stack).await, before);
    stack.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pending_migration_refuses_names_migrate_and_serve_does_not_migrate() {
    // Arrange: the newest migration is not recorded as applied.
    let stack = Stack::new().await;
    stack
        .root
        .db
        .admin_exec(
            "DELETE FROM _sqlx_migrations WHERE version = (SELECT max(version) FROM _sqlx_migrations)",
        )
        .await;
    let before = migrations(&stack).await;

    // Act
    let refused = stack.refused();

    // Assert
    assert_refused(&refused, &["pending", "sunnycloud migrate"]);
    assert_eq!(migrations(&stack).await, before);
    stack.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_master_key_file_refuses_with_file_missing() {
    // Arrange
    let mut stack = Stack::new().await;
    let absent = stack.root.setup.dir.join("absent.key");
    stack
        .root
        .setup
        .secret("master_key_file", &absent.display().to_string());

    // Act
    let refused = stack.refused();

    // Assert
    assert_refused(&refused, &["FileMissing", "master_key_file"]);
    stack.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_31_byte_master_key_refuses_with_wrong_length_and_prints_no_key_byte() {
    // Arrange: 31 bytes of a marker no other output contains.
    let mut stack = Stack::new().await;
    let key = stack.root.setup.dir.join("short.key");
    fs::write(&key, [b'Q'; 31]).expect("key file");
    stack
        .root
        .setup
        .secret("master_key_file", &key.display().to_string());

    // Act
    let refused = stack.refused();

    // Assert
    assert_refused(&refused, &["WrongLength"]);
    let output = format!("{}{}", refused.exited.stderr_text(), refused.exited.stdout);
    assert!(!output.contains("QQQQ"), "{output}");
    stack.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_volume_with_blobs_and_no_keycheck_refuses() {
    // Arrange: a blob file on a volume whose `.keycheck` was never written.
    let stack = Stack::new().await;
    let blob = sunnycloud_blobstore::blob_path(&stack.blob_root(), Uuid::new_v4());
    fs::create_dir_all(blob.parent().expect("blob folder")).expect("blob folder");
    fs::write(&blob, b"sealed bytes").expect("blob file");

    // Act
    let refused = stack.refused();

    // Assert
    assert_refused(&refused, &["KeyCheckAbsentButBlobsExist"]);
    assert!(!stack.blob_root().join(".keycheck").exists());
    stack.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_schema_owner_url_refuses_naming_the_role_check() {
    // Arrange
    let mut stack = Stack::new().await;
    let owner = stack
        .root
        .db
        .config()
        .db_migrator_url
        .expect("migrator URL")
        .expose()
        .clone();
    stack.root.setup.secret("db_api_url", &owner);

    // Act
    let refused = stack.refused();

    // Assert
    assert_refused(&refused, &["database role check"]);
    let messages = refused.exited.messages();
    assert!(
        !messages
            .iter()
            .any(|message| message == "serve start: database role"),
        "{messages:#?}"
    );
    stack.finish().await;
}
