//! Security NFR of AZ-3970: the fixture and its test CA never reach a release build.
//! The fixture lives under `tests/`; its crates are dev-dependencies only and no certificate is in the source.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The CA generator and the TLS stack the fixture needs; none may be reachable from the library's normal tree.
const FIXTURE_ONLY: &[&str] = &["rcgen", "tokio-rustls", "http-body-util", "hyper-util"];

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
        .filter(|name| FIXTURE_ONLY.contains(name))
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
