//! AC-5: the fault hook cannot be enabled or reached in a release build. The `sync-hook` feature is declared once,
//! is not a default, and is turned on only by the crate's own dev-dependency; no release tree enables it.

use std::fs;
use std::path::Path;
use std::process::Command;

const FEATURE: &str = "sync-hook";

fn manifest() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("manifest is readable")
}

fn blobstore_features(edges: &str) -> Vec<String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args(["tree", "--locked", "--workspace", "--edges", edges])
        .args(["--prefix", "none", "--format", "{p} [{f}]"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree runs");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("sunnycloud-blobstore "))
        .map(str::to_owned)
        .collect()
}

#[test]
fn no_normal_or_build_tree_of_the_workspace_enables_the_hook() {
    let release = blobstore_features("normal,build");

    assert!(!release.is_empty(), "the crate is in the workspace tree");
    assert!(
        release.iter().all(|line| !line.contains(FEATURE)),
        "release tree enables the hook: {release:?}"
    );
}

#[test]
fn the_check_sees_the_hook_when_dev_dependencies_are_included() {
    let with_dev = blobstore_features("normal,build,dev");

    assert!(
        with_dev.iter().any(|line| line.contains(FEATURE)),
        "the tree check cannot detect the feature: {with_dev:?}"
    );
}

#[test]
fn the_feature_is_declared_once_outside_default_and_used_only_by_the_self_dev_dependency() {
    let manifest = manifest();

    let lines: Vec<(usize, &str)> = manifest
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with('#') && line.contains(FEATURE))
        .collect();
    let dev_section = manifest
        .lines()
        .position(|line| line.trim() == "[dev-dependencies]")
        .expect("the manifest has dev-dependencies");

    assert_eq!(lines.len(), 2, "unexpected uses: {lines:?}");
    assert!(lines[0].1.starts_with(FEATURE), "declaration: {lines:?}");
    assert!(
        lines[1].0 > dev_section && lines[1].1.starts_with("sunnycloud-blobstore"),
        "use outside the self dev-dependency: {lines:?}"
    );
    assert!(
        !manifest.contains("default ="),
        "a default feature list could enable the hook"
    );
}

#[test]
fn no_other_manifest_of_the_workspace_names_the_feature() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let found = Command::new("grep")
        .args(["-rl", "--include=Cargo.toml", FEATURE])
        .arg(&src)
        .output()
        .expect("grep runs");

    let files = String::from_utf8_lossy(&found.stdout);
    assert!(
        files
            .lines()
            .all(|path| path.ends_with("04_blobstore/Cargo.toml") && !path.contains("target")),
        "other manifests name the feature: {files}"
    );
}

#[test]
fn the_public_api_exports_the_hook_only_behind_the_feature() {
    let lib = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
        .expect("lib.rs is readable");

    let lines: Vec<&str> = lib.lines().collect();
    let exports: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains("SyncHook"))
        .map(|(index, _)| index)
        .collect();

    assert!(!exports.is_empty());
    for index in exports {
        assert!(
            lines[index - 1].contains(&format!("cfg(feature = \"{FEATURE}\")")),
            "unguarded export: {}",
            lines[index]
        );
    }
}
