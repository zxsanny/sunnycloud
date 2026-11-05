use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("source directory is readable") {
        let path = entry.expect("directory entry is readable").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "target") {
                rust_files(&path, found);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn no_placeholder_bodies_in_source() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("server crate sits inside the workspace");
    let needles = [["todo", "!("].concat(), ["unimplemented", "!("].concat()];
    let mut files = Vec::new();
    rust_files(workspace, &mut files);

    let hits: Vec<_> = files
        .iter()
        .filter(|file| {
            let text = fs::read_to_string(file).expect("rust file is readable");
            needles.iter().any(|needle| text.contains(needle))
        })
        .collect();

    assert!(hits.is_empty(), "placeholder macros in: {hits:?}");
}

#[test]
fn every_crate_inherits_the_workspace_lints() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("server crate sits inside the workspace");
    let mut manifests = Vec::new();
    manifest_files(workspace, &mut manifests);

    let missing: Vec<_> = manifests
        .iter()
        .filter(|file| **file != workspace.join("Cargo.toml"))
        .filter(|file| {
            let text = fs::read_to_string(file).expect("manifest is readable");
            !text.contains("[lints]\nworkspace = true")
        })
        .collect();

    assert!(
        missing.is_empty(),
        "manifests without workspace lints: {missing:?}"
    );
}

fn manifest_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("source directory is readable") {
        let path = entry.expect("directory entry is readable").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "target") {
                manifest_files(&path, found);
            }
        } else if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            found.push(path);
        }
    }
}
