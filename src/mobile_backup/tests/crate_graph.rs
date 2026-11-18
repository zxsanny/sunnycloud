//! AC-1: the shipped dependency tree of `mobile_backup` (normal and build edges, with and without `uniffi`) holds
//! sync-core and the helper crates of this repository only: no `contracts`, no server or component crate, no
//! `vault-core`.
#![cfg(not(target_arch = "wasm32"))]

use std::process::Command;

/// Crates of this repository the library may depend on: `sync-core`, its protocol crate and the helpers (`sc_*`).
const LOCAL_ALLOWED: &[&str] = &["mobile_backup", "sync-core", "sync-core-proto"];

fn cargo_tree(features: &str) -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut args = vec![
        "tree",
        "--locked",
        "--package",
        "mobile_backup",
        "--edges",
        "normal,build",
        "--prefix",
        "none",
    ];
    if !features.is_empty() {
        args.extend(["--features", features]);
    }
    let output = Command::new(cargo)
        .args(&args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree did not start");
    assert!(
        output.status.success(),
        "cargo tree {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("cargo tree output is UTF-8")
}

/// `(name, local path?)` per tree line; a path source marks a crate of this repository.
fn packages(tree: &str) -> Vec<(&str, bool)> {
    tree.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let local = parts.any(|part| part.starts_with("(/"));
            Some((name, local))
        })
        .collect()
}

fn violations(tree: &str) -> Vec<String> {
    let mut found: Vec<String> = packages(tree)
        .into_iter()
        .filter_map(|(name, local)| {
            let forbidden_name = matches!(name, "contracts" | "vault-core")
                || name.starts_with("sunnycloud")
                || name == "sc_db"
                || name == "sc_testdb";
            let helper = name.starts_with("sc_");
            if forbidden_name || (local && !helper && !LOCAL_ALLOWED.contains(&name)) {
                Some(name.to_owned())
            } else {
                None
            }
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

#[test]
fn the_default_tree_holds_sync_core_and_helpers_only() {
    // Arrange
    let tree = cargo_tree("");

    // Act
    let found = violations(&tree);

    // Assert
    assert!(found.is_empty(), "forbidden crates: {found:?}");
    assert!(
        packages(&tree).iter().any(|(name, _)| *name == "sync-core"),
        "sync-core is missing from the tree"
    );
}

#[test]
fn the_uniffi_tree_holds_sync_core_and_helpers_only() {
    // Arrange
    let tree = cargo_tree("uniffi");

    // Act
    let found = violations(&tree);

    // Assert
    assert!(found.is_empty(), "forbidden crates: {found:?}");
}

#[test]
fn the_check_names_a_forbidden_crate_when_one_is_in_the_tree() {
    // Arrange
    let tree = "mobile_backup v0.1.0 (/repo/src/mobile_backup)\ncontracts v0.1.0 (/repo/src/contracts)\n\
                vault-core v0.1.0 (/repo/src/vault-core)\nsunnycloud-server v0.1.0 (/repo/src/server)\n\
                sc_config v0.1.0 (/repo/src/shared/config)\nserde v1.0.0\n";

    // Act
    let found = violations(tree);

    // Assert
    assert_eq!(found, ["contracts", "sunnycloud-server", "vault-core"]);
}
