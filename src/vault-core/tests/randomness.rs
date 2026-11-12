//! AC-3: no code outside the native OS-source module reads system randomness, and `getrandom` is an optional
//! dependency enabled only by the `os-rng` feature.
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

const OS_SOURCE_FILE: &str = "os_random.rs";
const SYSTEM_RANDOMNESS: &[&str] = &[
    "getrandom",
    "OsRng",
    "SysRng",
    "thread_rng",
    "rand::",
    "/dev/urandom",
    "getRandomValues(",
    "Math.random",
];

fn rust_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("source directory is readable") {
        let path = entry.expect("directory entry is readable").path();
        if path.is_dir() {
            rust_sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

fn randomness_reads(file_name: &str, source: &str) -> Vec<String> {
    if file_name == OS_SOURCE_FILE {
        return Vec::new();
    }
    SYSTEM_RANDOMNESS
        .iter()
        .filter(|token| source.contains(*token))
        .map(|token| format!("{file_name} reads system randomness through {token}"))
        .collect()
}

fn manifest_violations(manifest: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with("getrandom") && !line.contains("optional = true") {
            found.push("getrandom is not optional".to_string());
        }
        if line.contains("dep:getrandom") && !line.starts_with("os-rng =") {
            found.push(format!("feature enables getrandom: {line}"));
        }
        let feature_forwards_rng =
            line.contains("/getrandom") || line.contains("/os_rng") || line.contains("/rand_core");
        if feature_forwards_rng {
            found.push(format!("dependency randomness feature: {line}"));
        }
    }
    found
}

#[test]
fn only_the_os_source_reads_system_randomness() {
    // Arrange
    let mut files = Vec::new();
    rust_sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );

    // Act
    let found: Vec<String> = files
        .iter()
        .flat_map(|path| {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let source = std::fs::read_to_string(path).expect("source file is readable");
            randomness_reads(name, &source)
        })
        .collect();

    // Assert
    assert!(files.len() >= 5, "source scan found too few files");
    assert!(
        found.is_empty(),
        "system randomness outside {OS_SOURCE_FILE}: {found:?}"
    );
}

#[test]
fn seeded_randomness_read_outside_the_os_source_fails() {
    // Arrange
    let seeded = "fn salt() -> [u8; 16] { let mut s = [0; 16]; getrandom::fill(&mut s); s }";

    // Act
    let outside = randomness_reads("header.rs", seeded);
    let inside = randomness_reads(OS_SOURCE_FILE, seeded);

    // Assert
    assert_eq!(
        outside,
        vec!["header.rs reads system randomness through getrandom"]
    );
    assert!(inside.is_empty());
}

#[test]
fn manifest_enables_getrandom_only_through_the_os_feature() {
    // Arrange
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("Cargo.toml is readable");

    // Act
    let found = manifest_violations(&manifest);

    // Assert
    assert!(found.is_empty(), "manifest violations: {found:?}");
}

#[test]
fn seeded_manifest_randomness_fails() {
    // Arrange
    let seeded = "default = [\"dep:getrandom\"]\n\
                  getrandom = { workspace = true }\n\
                  chacha20poly1305 = { version = \"=0.11.0\", features = [\"alloc\"] }\n\
                  x = [\"chacha20poly1305/getrandom\"]\n";

    // Act
    let found = manifest_violations(seeded);

    // Assert
    assert_eq!(found.len(), 3, "{found:?}");
}
