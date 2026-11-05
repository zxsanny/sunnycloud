//! AC-1: the crate's dependency tree, native and `wasm32-unknown-unknown`, holds no runtime, HTTP, server
//! or other client crate. Same method as `server/tests/wasm_split.rs`.

use std::process::Command;

/// Crates this protocol crate must never reach: HTTP and IO runtimes, the native client, the WASM adapter,
/// the server-only `contracts` and the server components.
const NEVER: &[&str] = &[
    "reqwest",
    "mio",
    "hyper",
    "sync-core",
    "sync-core-wasm",
    "contracts",
    "media-shared",
    "sunnycloud-identity",
    "sunnycloud-tree",
    "sunnycloud-transfer",
    "sunnycloud-blobstore",
    "sunnycloud-sync-feed",
    "sunnycloud-sharing",
    "sunnycloud-admin",
    "sunnycloud-quota",
    "media-api",
    "sunnycloud-http-api",
    "sunnycloud-vault-service",
    "sunnycloud-server",
    "sunnycloud-worker",
    "sqlx",
];

/// The clock and the async runtime (AC-1 "no tokio ... or clock crate").
const CLOCK_AND_RUNTIME: &[&str] = &["tokio", "sc_clock"];

fn offenders(tree: &str, forbidden: &[&str]) -> Vec<String> {
    let mut found: Vec<String> = tree
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| forbidden.contains(name))
        .map(str::to_string)
        .collect();
    found.sort();
    found.dedup();
    found
}

fn tree(target: Option<&str>) -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = Command::new(cargo);
    command.args([
        "tree",
        "--locked",
        "--package",
        "sync-core-proto",
        "--edges",
        "normal,build",
        "--prefix",
        "none",
    ]);
    if let Some(target) = target {
        command.args(["--target", target]);
    }
    let output = command
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree did not start");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("cargo tree output is UTF-8")
}

#[test]
fn native_tree_has_no_http_server_or_other_client_crate() {
    assert_eq!(offenders(&tree(None), NEVER), Vec::<String>::new());
}

#[test]
fn wasm_tree_has_no_http_server_or_other_client_crate() {
    assert_eq!(
        offenders(&tree(Some("wasm32-unknown-unknown")), NEVER),
        Vec::<String>::new()
    );
}

#[test]
fn seeded_forbidden_crates_in_a_tree_are_found() {
    let seeded =
        "sync-core-proto v0.1.0 (/w)\nreqwest v0.13.5\nmio v1.0.0\ntokio v1.54.1\nuuid v1.26.1\n";

    assert_eq!(offenders(seeded, NEVER), vec!["mio", "reqwest"]);
    assert_eq!(offenders(seeded, CLOCK_AND_RUNTIME), vec!["tokio"]);
}

/// AC-1: no tokio and no clock crate in the tree. `sc_ids` is built without its `new-id` feature and
/// `sc_clock` is not reached at all.
#[test]
fn trees_have_no_tokio_or_clock_crate() {
    assert_eq!(
        offenders(&tree(None), CLOCK_AND_RUNTIME),
        Vec::<String>::new()
    );
    assert_eq!(
        offenders(&tree(Some("wasm32-unknown-unknown")), CLOCK_AND_RUNTIME),
        Vec::<String>::new()
    );
}
