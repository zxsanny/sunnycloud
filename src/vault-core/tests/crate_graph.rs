//! AC-1, AC-3, AC-5: the shipped dependency trees of `vault-core` (normal and build edges, per feature set
//! and target) hold no sunnycloud crate, no `contracts`, no I/O or network crate, no `getrandom` unless the
//! native OS source is on, and the crypto crates at their exact pins.
#![cfg(not(target_arch = "wasm32"))]

use std::process::Command;

const NETWORK: &[&str] = &[
    "tokio",
    "mio",
    "async-std",
    "smol",
    "reqwest",
    "hyper",
    "h2",
    "ureq",
    "curl",
    "socket2",
    "rustls",
    "native-tls",
    "openssl",
];
const FILE_IO: &[&str] = &[
    "tempfile", "fs-err", "memmap2", "rusqlite", "sqlx", "notify",
];
const PINS: &[(&str, &str)] = &[
    ("chacha20poly1305", "v0.11.0"),
    ("aead-stream", "v0.6.0"),
    ("argon2", "v0.6.0"),
    ("blake3", "v1.8.7"),
];
const WASM: &str = "wasm32-unknown-unknown";

fn cargo_tree(features: &str, target: Option<&str>) -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut args = vec![
        "tree",
        "--locked",
        "--package",
        "vault-core",
        "--edges",
        "normal,build",
        "--prefix",
        "none",
        "--no-default-features",
    ];
    if !features.is_empty() {
        args.extend(["--features", features]);
    }
    if let Some(target) = target {
        args.extend(["--target", target]);
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

/// `(name, version, local path?)` per tree line; a path source marks a crate of this repository.
fn packages(tree: &str) -> Vec<(&str, &str, bool)> {
    tree.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let version = parts.next().unwrap_or("");
            let local = parts.any(|part| part.starts_with("(/") || part.contains(":\\"));
            Some((name, version, local))
        })
        .collect()
}

fn violations(tree: &str, forbidden: &[&[&str]]) -> Vec<String> {
    let mut found: Vec<String> = packages(tree)
        .into_iter()
        .filter_map(|(name, _, local)| {
            let sunnycloud = (local && name != "vault-core")
                || name == "contracts"
                || name.starts_with("sc_")
                || name.starts_with("sunnycloud");
            if sunnycloud {
                Some(format!("sunnycloud crate {name}"))
            } else if forbidden.iter().any(|list| list.contains(&name)) {
                Some(format!("forbidden crate {name}"))
            } else {
                None
            }
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

fn pin_mismatches(tree: &str) -> Vec<String> {
    let found = packages(tree);
    PINS.iter()
        .filter(|(pin, version)| {
            let versions: Vec<&str> = found
                .iter()
                .filter(|(name, _, _)| name == pin)
                .map(|(_, found_version, _)| *found_version)
                .collect();
            versions.is_empty()
                || versions
                    .iter()
                    .any(|found_version| found_version != version)
        })
        .map(|(pin, version)| format!("{pin} is not at {version}"))
        .collect()
}

fn has(tree: &str, crate_name: &str) -> bool {
    packages(tree)
        .iter()
        .any(|(name, _, _)| *name == crate_name)
}

#[test]
fn native_core_tree_is_pure_and_has_no_system_randomness() {
    // Arrange
    let tree = cargo_tree("", None);

    // Act
    let found = violations(&tree, &[NETWORK, FILE_IO]);

    // Assert
    assert!(found.is_empty(), "violations: {found:?}");
    assert!(!has(&tree, "getrandom"), "getrandom in the core tree");
}

#[test]
fn native_tree_with_policy_and_os_source_is_pure() {
    // Arrange
    let tree = cargo_tree("passphrase-policy,os-rng", None);

    // Act
    let found = violations(&tree, &[NETWORK, FILE_IO]);

    // Assert
    assert!(found.is_empty(), "violations: {found:?}");
}

#[test]
fn wasm_tree_is_pure_and_has_no_getrandom() {
    // Arrange
    let tree = cargo_tree("wasm,passphrase-policy", Some(WASM));

    // Act
    let found = violations(&tree, &[NETWORK, FILE_IO]);

    // Assert
    assert!(found.is_empty(), "violations: {found:?}");
    assert!(!has(&tree, "getrandom"), "getrandom in the WASM tree");
}

/// UniFFI 0.32.2 itself depends on `tempfile` (through `uniffi_pipeline`), so only the network list applies.
#[test]
fn uniffi_tree_has_no_sunnycloud_or_network_crate() {
    // Arrange
    let tree = cargo_tree("uniffi,os-rng,passphrase-policy", None);

    // Act
    let found = violations(&tree, &[NETWORK]);

    // Assert
    assert!(found.is_empty(), "violations: {found:?}");
}

#[test]
fn seeded_sunnycloud_contracts_and_io_crates_fail_the_check() {
    // Arrange
    let tree = "vault-core v0.1.0 (/w/src/vault-core)\n\
                blake3 v1.8.7\n\
                sc_ids v0.1.0 (/w/src/shared/ids_hashing)\n\
                contracts v0.1.0 (/w/src/contracts)\n\
                local-helper v0.1.0 (/w/src/helper) (proc-macro)\n\
                tokio v1.54.1\n\
                tempfile v3.27.0\n";

    // Act
    let found = violations(tree, &[NETWORK, FILE_IO]);

    // Assert
    assert_eq!(
        found,
        vec![
            "forbidden crate tempfile",
            "forbidden crate tokio",
            "sunnycloud crate contracts",
            "sunnycloud crate local-helper",
            "sunnycloud crate sc_ids",
        ]
    );
}

#[test]
fn crypto_crates_are_at_their_exact_pins_without_rayon() {
    // Arrange
    let tree = cargo_tree("passphrase-policy,os-rng", None);

    // Act
    let mismatches = pin_mismatches(&tree);

    // Assert
    assert!(mismatches.is_empty(), "pin mismatches: {mismatches:?}");
    assert!(!has(&tree, "rayon"), "argon2 or blake3 with parallel");
}

#[test]
fn seeded_pin_drift_fails_the_check() {
    // Arrange
    let tree = "chacha20poly1305 v0.10.1\n\
                aead-stream v0.6.0\n\
                blake3 v1.8.7\n";

    // Act
    let mismatches = pin_mismatches(tree);

    // Assert
    assert_eq!(
        mismatches,
        vec![
            "chacha20poly1305 is not at v0.11.0",
            "argon2 is not at v0.6.0"
        ]
    );
}
