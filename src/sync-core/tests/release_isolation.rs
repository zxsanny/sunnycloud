//! Security NFR of AZ-3970 and AC-7 of AZ-3959: the fixture, its test CA and the test-only entry points of the
//! client never reach a release build. The fixture lives under `tests/`; its crates are dev-dependencies only and
//! no certificate is in the source. The client's `allow-plain-http` and `test-support` features are absent from
//! every normal and build tree of the workspace, which is what a release binary links.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The CA generator and the TLS stack the fixture needs; each must be declared as a dev-dependency.
const FIXTURE_ONLY: &[&str] = &["rcgen", "tokio-rustls", "http-body-util", "hyper-util"];

/// The only fixture crate the client itself never needs: reqwest brings `hyper-util` and `tokio-rustls` into the
/// release tree, so those two are checked in the manifest only.
const NOT_IN_RELEASE_TREE: &[&str] = &["rcgen"];

/// Features of `sync-core` that a release tree must not contain.
const DEBUG_ONLY_FEATURES: &[&str] = &["allow-plain-http", "test-support"];

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("source directory is readable") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn the_normal_dependency_tree_holds_no_fixture_crate() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());

    let output = Command::new(cargo)
        .args([
            "tree",
            "--locked",
            "--package",
            "sync-core",
            "--edges",
            "normal,build",
            "--prefix",
            "none",
        ])
        .output()
        .expect("cargo tree runs");

    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8_lossy(&output.stdout);
    let found: Vec<&str> = tree
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| NOT_IN_RELEASE_TREE.contains(name))
        .collect();
    assert!(
        found.is_empty(),
        "fixture crates in the release tree: {found:?}"
    );
}

#[test]
fn no_certificate_or_key_is_stored_in_the_library_source() {
    let mut files = Vec::new();
    rust_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let needles = [
        ["BEGIN ", "CERTIFICATE"].concat(),
        ["PRIVATE", " KEY"].concat(),
        "rcgen".to_owned(),
    ];

    let hits: Vec<_> = files
        .iter()
        .filter(|file| {
            let text = fs::read_to_string(file).expect("rust file is readable");
            needles.iter().any(|needle| text.contains(needle))
        })
        .collect();

    assert!(
        hits.is_empty(),
        "test CA material in library source: {hits:?}"
    );
}

#[test]
fn fixture_dependencies_are_declared_as_dev_dependencies_only() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("manifest is readable");
    let (normal, dev) = manifest
        .split_once("[dev-dependencies]")
        .expect("the manifest has a dev-dependencies section");

    for name in FIXTURE_ONLY {
        assert!(
            !normal.contains(name),
            "{name} declared outside dev-dependencies"
        );
        assert!(dev.contains(name), "{name} should be a dev-dependency");
    }
}

/// `sync-core` lines of `cargo tree` over the whole workspace as a release links it (no dev edges),
/// with `extra` arguments to seed a build.
fn sync_core_tree_lines(extra: &[&str]) -> Vec<String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args([
            "tree",
            "--locked",
            "--workspace",
            "--edges",
            "normal,build",
            "--prefix",
            "none",
            "--format",
            "{p} [{f}]",
        ])
        .args(extra)
        .output()
        .expect("cargo tree runs");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("sync-core "))
        .map(str::to_owned)
        .collect()
}

/// The debug-only features present in `lines`; the release feature check fails when this is not empty.
fn debug_only_features_in(lines: &[String]) -> Vec<&'static str> {
    DEBUG_ONLY_FEATURES
        .iter()
        .copied()
        .filter(|feature| {
            lines.iter().any(|line| {
                line.rsplit_once('[').is_some_and(|(_, features)| {
                    features
                        .trim_end_matches(']')
                        .split(',')
                        .any(|name| name == *feature)
                })
            })
        })
        .collect()
}

#[test]
fn the_release_tree_has_no_plain_http_or_test_support_feature() {
    let lines = sync_core_tree_lines(&[]);

    assert!(!lines.is_empty(), "sync-core is not in the workspace tree");
    assert_eq!(debug_only_features_in(&lines), Vec::<&str>::new());
}

#[test]
fn the_feature_check_fails_on_a_seeded_build_with_plain_http() {
    let lines = sync_core_tree_lines(&["--features", "sync-core/allow-plain-http"]);

    assert_eq!(debug_only_features_in(&lines), vec!["allow-plain-http"]);
}

#[test]
fn the_feature_check_fails_on_a_seeded_build_with_test_support() {
    let lines = sync_core_tree_lines(&["--features", "sync-core/test-support"]);

    assert_eq!(debug_only_features_in(&lines), vec!["test-support"]);
}

#[test]
fn a_release_profile_build_refuses_plain_http_at_compile_time() {
    let lib = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
        .expect("lib.rs is readable");

    assert!(lib.contains(
        "#[cfg(all(feature = \"allow-plain-http\", not(debug_assertions)))]\ncompile_error!"
    ));
}
