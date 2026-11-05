use std::process::Command;

const FORBIDDEN: &[&str] = &["tokio", "reqwest", "mio", "sync-core"];

fn forbidden_in_tree(tree: &str) -> Vec<String> {
    let mut found: Vec<String> = tree
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| FORBIDDEN.contains(name))
        .map(str::to_string)
        .collect();
    found.sort();
    found.dedup();
    found
}

fn wasm_tree() -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args([
            "tree",
            "--locked",
            "--package",
            "sync-core-wasm",
            "--target",
            "wasm32-unknown-unknown",
            "--edges",
            "normal,build",
            "--prefix",
            "none",
        ])
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
fn wasm_adapter_tree_has_no_native_client_dependency() {
    let tree = wasm_tree();

    let found = forbidden_in_tree(&tree);

    assert!(
        found.is_empty(),
        "forbidden crates in the wasm tree: {found:?}"
    );
}

#[test]
fn seeded_native_crates_in_the_tree_are_rejected() {
    let tree = "sync-core-wasm v0.1.0 (/w/sync-core-wasm)\n\
                wasm-bindgen v0.2.130\n\
                sync-core v0.1.0 (/w/sync-core)\n\
                tokio v1.54.1\n\
                mio v1.0.0\n\
                reqwest v0.13.5\n";

    let found = forbidden_in_tree(tree);

    assert_eq!(found, vec!["mio", "reqwest", "sync-core", "tokio"]);
}
