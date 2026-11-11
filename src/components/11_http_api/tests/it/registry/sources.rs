use std::fs;
use std::path::{Path, PathBuf};

const ROUTER_ENTRY_POINTS: [&str; 5] = [
    ".route(",
    ".route_service(",
    ".nest(",
    ".nest_service(",
    "Router::merge",
];
/// `test_clock.rs` builds the one router of the `:9091` test-clock listener, which is outside the registry and the
/// OpenAPI document (`api_v1_surface.md`) and compiled only with feature `test-clock`.
const ALLOWED: [&str; 2] = ["registry/mount.rs", "test_clock.rs"];

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
fn no_route_is_added_to_a_router_except_through_the_registry() {
    // Arrange
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&source, &mut files);

    // Act
    let offenders: Vec<String> = files
        .iter()
        .filter(|file| !ALLOWED.iter().any(|allowed| file.ends_with(allowed)))
        .filter(|file| {
            let text = fs::read_to_string(file).expect("rust file is readable");
            ROUTER_ENTRY_POINTS
                .iter()
                .any(|needle| text.contains(needle))
        })
        .map(|file| file.display().to_string())
        .collect();

    // Assert
    assert!(files.len() > 10);
    assert_eq!(offenders, Vec::<String>::new());
}
