//! AC-1 and AC-5 of AZ-3968 (`uniffi_surface.md` feature-off and one-version cases): without the `uniffi`
//! feature no UniFFI crate is in the shipped trees of `sync-core`, `sync-core-proto`, `sync-core-wasm` (native
//! and WASM) or the desktop daemon; with it, every UniFFI crate is at 0.32.2; the lockfile holds one `uniffi`
//! and no second `syn` of the same major version.

use std::path::Path;
use std::process::Command;

const UNIFFI_VERSION: &str = "0.32.2";
const WASM: &str = "wasm32-unknown-unknown";

/// `cargo tree` of `package` as a release links it (normal and build edges, default features), plus `extra`.
fn cargo_tree(package: &str, extra: &[&str]) -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args([
            "tree",
            "--locked",
            "--package",
            package,
            "--edges",
            "normal,build",
            "--prefix",
            "none",
        ])
        .args(extra)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree did not start");
    assert!(
        output.status.success(),
        "cargo tree -p {package} {extra:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("cargo tree output is UTF-8")
}

/// `(name, version)` of every UniFFI crate in a tree, without duplicates.
fn uniffi_crates(tree: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = tree
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let version = parts.next().unwrap_or("").trim_start_matches('v');
            name.starts_with("uniffi")
                .then(|| (name.to_owned(), version.to_owned()))
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Versions of `name` in a `Cargo.lock` text.
fn locked_versions(lock: &str, name: &str) -> Vec<String> {
    let wanted = format!("name = \"{name}\"");
    lock.split("[[package]]")
        .filter(|package| package.lines().any(|line| line.trim() == wanted))
        .filter_map(|package| {
            package
                .lines()
                .find_map(|line| line.trim().strip_prefix("version = "))
                .map(|version| version.trim_matches('"').to_owned())
        })
        .collect()
}

/// Major versions that occur more than once among `versions` (`0.x` counts by its minor).
fn duplicate_majors(versions: &[String]) -> Vec<String> {
    let major = |version: &String| {
        let mut parts = version.split('.');
        match (parts.next(), parts.next()) {
            (Some("0"), Some(minor)) => format!("0.{minor}"),
            (Some(major), _) => major.to_owned(),
            (None, _) => String::new(),
        }
    };
    let mut majors: Vec<String> = versions.iter().map(major).collect();
    majors.sort();
    let mut duplicates: Vec<String> = majors
        .windows(2)
        .filter(|pair| pair[0] == pair[1])
        .map(|pair| pair[0].clone())
        .collect();
    duplicates.dedup();
    duplicates
}

fn lockfile() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.lock");
    std::fs::read_to_string(path).expect("the workspace lockfile is readable")
}

#[test]
fn feature_off_native_trees_hold_no_uniffi_crate() {
    // Arrange
    let packages = [
        "sync-core",
        "sync-core-proto",
        "sync-core-wasm",
        "sunnycloud-daemon",
    ];

    // Act
    let found: Vec<(&str, Vec<(String, String)>)> = packages
        .iter()
        .map(|package| (*package, uniffi_crates(&cargo_tree(package, &[]))))
        .filter(|(_, crates)| !crates.is_empty())
        .collect();

    // Assert
    assert!(
        found.is_empty(),
        "UniFFI crates without the feature: {found:?}"
    );
}

#[test]
fn feature_off_wasm_trees_hold_no_uniffi_crate() {
    // Arrange
    let packages = ["sync-core-proto", "sync-core-wasm"];

    // Act
    let found: Vec<(&str, Vec<(String, String)>)> = packages
        .iter()
        .map(|package| {
            let tree = cargo_tree(package, &["--target", WASM]);
            (*package, uniffi_crates(&tree))
        })
        .filter(|(_, crates)| !crates.is_empty())
        .collect();

    // Assert
    assert!(
        found.is_empty(),
        "UniFFI crates in the WASM trees: {found:?}"
    );
}

#[test]
fn feature_on_tree_holds_uniffi_at_its_pin_only() {
    // Arrange
    let tree = cargo_tree("sync-core", &["--features", "uniffi"]);

    // Act
    let crates = uniffi_crates(&tree);
    let off_pin: Vec<&(String, String)> = crates
        .iter()
        .filter(|(_, version)| version != UNIFFI_VERSION)
        .collect();

    // Assert
    assert!(
        crates.iter().any(|(name, _)| name == "uniffi"),
        "the feature does not bring uniffi: {crates:?}"
    );
    assert!(off_pin.is_empty(), "UniFFI crates off the pin: {off_pin:?}");
}

#[test]
fn lockfile_holds_one_uniffi_and_one_syn_per_major() {
    // Arrange
    let lock = lockfile();

    // Act
    let uniffi = locked_versions(&lock, "uniffi");
    let syn = locked_versions(&lock, "syn");

    // Assert
    assert_eq!(uniffi, vec![UNIFFI_VERSION.to_owned()]);
    assert!(!syn.is_empty(), "no syn in the lockfile");
    assert_eq!(duplicate_majors(&syn), Vec::<String>::new(), "syn: {syn:?}");
}

#[test]
fn seeded_uniffi_crates_are_found_in_a_tree() {
    // Arrange
    let tree = "sync-core v0.1.0 (/w/src/sync-core)\n\
                uniffi v0.31.0\n\
                uniffi_core v0.32.2\n\
                uniffi_core v0.32.2 (*)\n\
                tokio v1.54.1\n";

    // Act
    let crates = uniffi_crates(tree);

    // Assert
    assert_eq!(
        crates,
        vec![
            ("uniffi".to_owned(), "0.31.0".to_owned()),
            ("uniffi_core".to_owned(), UNIFFI_VERSION.to_owned()),
        ]
    );
}

#[test]
fn seeded_second_uniffi_and_second_syn_fail_the_lockfile_check() {
    // Arrange
    let lock = "[[package]]\nname = \"uniffi\"\nversion = \"0.32.2\"\n\n\
                [[package]]\nname = \"uniffi\"\nversion = \"0.31.0\"\n\n\
                [[package]]\nname = \"syn\"\nversion = \"2.0.39\"\n\n\
                [[package]]\nname = \"syn\"\nversion = \"2.0.119\"\n\n\
                [[package]]\nname = \"syn\"\nversion = \"3.0.9\"\n";

    // Act
    let uniffi = locked_versions(lock, "uniffi");
    let duplicates = duplicate_majors(&locked_versions(lock, "syn"));

    // Assert
    assert_eq!(uniffi, vec!["0.32.2".to_owned(), "0.31.0".to_owned()]);
    assert_eq!(duplicates, vec!["2".to_owned()]);
}
