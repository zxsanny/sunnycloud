//! The test tooling is never linked into the settings binary (task constraint). The
//! settings crate may use it as a dev-dependency (its tests drive the stub daemon), but
//! never as a normal or build dependency.

use std::path::Path;

/// The lines of every `[dependencies]`-like table of a manifest that ships: normal and
/// build dependencies, also under `[target.*]`.
fn shipped_dependency_lines(manifest: &str) -> Vec<&str> {
    let mut shipped = false;
    let mut lines = Vec::new();
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            let table = trimmed.trim_matches(|c| c == '[' || c == ']');
            shipped = table.ends_with("dependencies") && !table.ends_with("dev-dependencies");
        } else if shipped {
            lines.push(line);
        }
    }
    lines
}

#[test]
fn the_settings_binary_does_not_depend_on_the_testkit() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");

    let text = std::fs::read_to_string(&manifest).expect("settings manifest is readable");

    let offending: Vec<&str> = shipped_dependency_lines(&text)
        .into_iter()
        .filter(|line| line.contains("testkit") || line.contains("test-tools"))
        .collect();
    assert!(
        offending.is_empty(),
        "settings ships a dependency on the testkit: {offending:?}"
    );
}

#[test]
fn the_check_tells_a_shipped_dependency_from_a_dev_dependency() {
    let manifest = "[dependencies]\nipc_schema = { path = \"x\" }\n\n[target.'cfg(unix)'.dependencies]\nsunnycloud-settings-testkit = { path = \"test-tools\" }\n\n[dev-dependencies]\nsunnycloud-settings-testkit = { path = \"test-tools\" }\n";

    let shipped = shipped_dependency_lines(manifest);

    assert_eq!(
        shipped,
        [
            "ipc_schema = { path = \"x\" }",
            "",
            "sunnycloud-settings-testkit = { path = \"test-tools\" }",
            ""
        ]
    );
}
