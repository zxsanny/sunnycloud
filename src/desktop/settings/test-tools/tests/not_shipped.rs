//! The test tooling is never linked into the settings binary (task constraint).

use std::path::Path;

#[test]
fn the_settings_binary_does_not_depend_on_the_testkit() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml");

    let text = std::fs::read_to_string(&manifest).expect("settings manifest is readable");

    assert!(
        !text.contains("testkit"),
        "settings manifest names the testkit:\n{text}"
    );
    assert!(
        !text.contains("test-tools"),
        "settings manifest names test-tools:\n{text}"
    );
}
