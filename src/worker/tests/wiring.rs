//! AZ-3874 AC-2 (AZ-3661 AC-5): the production composition builds the slot loops with the real `WorkerApi`
//! client and `DecoderClient`, and no test double is reachable from non-test sources.

use std::fs;
use std::path::{Path, PathBuf};

const DOUBLES: [&str; 5] = ["Fake", "Mock", "Scripted", "Stub", "Passthrough"];

/// The `-test` build's clock (AZ-3875) is the one sanctioned name outside this list, and only in the files that
/// feature `test-clock` compiles.
const OFFSET_CLOCK: &str = "OffsetClock";
const OFFSET_CLOCK_FILES: [&str; 2] = ["clock_poll.rs", "clock_source.rs"];

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("list the sources") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// The text of a source file outside its `#[cfg(test)]` modules and outside files that only hold tests.
fn production_text(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if name == "tests.rs" || name.ends_with("_tests.rs") {
        return String::new();
    }
    let text = fs::read_to_string(path).expect("read a source");
    match text.find("#[cfg(test)]") {
        Some(at) => text[..at].to_owned(),
        None => text,
    }
}

fn all_production_text() -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    files
        .into_iter()
        .map(|path| {
            let text = production_text(&path);
            (path, text)
        })
        .collect()
}

#[test]
fn ac2_the_composition_builds_the_loops_from_the_real_client_decoder_slot_and_scratch() {
    // Arrange
    let compose = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/compose.rs");

    // Act
    let text = production_text(&compose);

    // Assert
    for built in [
        "WorkerApiClient::new(",
        "Slot::new(",
        "SlotLoop::new(",
        "ScratchRoot::new(",
    ] {
        assert!(text.contains(built), "compose.rs does not call {built}");
    }
}

#[test]
fn ac2_main_reaches_the_composition_through_run_worker_only() {
    // Arrange
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    // Act
    let run = production_text(&root.join("run.rs"));
    let lib = production_text(&root.join("lib.rs"));

    // Assert
    assert!(run.contains("assemble("), "run_worker does not assemble");
    assert!(lib.contains("serve(env)"), "execute does not serve");
}

#[test]
fn ac2_no_test_double_is_named_in_non_test_sources() {
    // Arrange
    let files = all_production_text();

    // Act
    let found: Vec<String> = files
        .iter()
        .flat_map(|(path, text)| {
            DOUBLES
                .iter()
                .filter(|double| text.contains(*double))
                .map(move |double| format!("{} names {double}", path.display()))
        })
        .collect();

    // Assert
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn az3875_the_offset_clock_is_named_only_in_files_the_test_clock_feature_compiles() {
    // Arrange
    let files = all_production_text();
    let lib = production_text(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"));

    // Act
    let found: Vec<String> = files
        .iter()
        .filter(|(path, text)| text.contains(OFFSET_CLOCK) && !is_offset_clock_file(path))
        .map(|(path, _)| path.display().to_string())
        .collect();

    // Assert
    assert!(found.is_empty(), "{found:?} name {OFFSET_CLOCK}");
    assert!(
        lib.contains("#[cfg(feature = \"test-clock\")]\nmod clock_poll;"),
        "the poll module is not gated by the feature"
    );
}

fn is_offset_clock_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| OFFSET_CLOCK_FILES.contains(&name))
}
