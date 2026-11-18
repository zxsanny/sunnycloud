//! The static wiring checks of the pipeline tasks (`src/worker/tests/wiring.rs` precedent): each flow operation, the
//! non-test file that calls or registers it, and the text that file must contain.

use std::fs;
use std::path::Path;

/// `(operation, file relative to the workspace, needles)`.
pub type Caller = (&'static str, &'static str, &'static [&'static str]);

/// The text of a source outside test files and `#[cfg(test)]` modules.
pub fn production_text(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if name == "tests.rs" || name.ends_with("_tests.rs") {
        return String::new();
    }
    let text =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    match text.find("#[cfg(test)]") {
        Some(at) => text[..at].to_owned(),
        None => text,
    }
}

/// The production text of a workspace source, named relative to the workspace root.
pub fn production_source(file: &str) -> String {
    production_text(&workspace().join(file))
}

fn workspace() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the server crate sits inside the workspace")
}

/// One line per needle that its file's production text lacks.
pub fn missing_callers(callers: &[Caller]) -> Vec<String> {
    let workspace = workspace();
    callers
        .iter()
        .flat_map(|(operation, file, needles)| {
            let text = production_text(&workspace.join(file));
            needles
                .iter()
                .filter(move |needle| !text.contains(**needle))
                .map(move |needle| format!("{operation}: {file} lacks `{needle}`"))
        })
        .collect()
}
