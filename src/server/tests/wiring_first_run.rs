//! AZ-3654 AC-5: every operation of the first-run flow (F1) has a production caller. The search covers the
//! non-test sources of the workspace; a caller that exists only in a test does not count.

use std::fs;
use std::path::{Path, PathBuf};

/// (operation, the call as it is written at the caller, the directory under `src/` where its caller lives).
const CALLERS: [(&str, &str, &str); 7] = [
    (
        "Migrator::run_pending",
        "Migrator::run_pending(",
        "server/src",
    ),
    (
        "Migrator::check_version",
        "Migrator::check_version(",
        "server/src",
    ),
    (
        "OperatorIdentityOps::ensure_setup_token",
        ".ensure_setup_token(",
        "server/src",
    ),
    (
        "OperatorIdentityOps::create_admin_invite",
        ".create_admin_invite(",
        "server/src",
    ),
    (
        "SignInService::list_providers",
        ".list_providers(",
        "components/11_http_api/src",
    ),
    (
        "SignInService::begin",
        ".begin(",
        "components/11_http_api/src/registry/signin",
    ),
    (
        "SignInService::complete",
        ".complete(",
        "components/11_http_api/src/registry/signin",
    ),
];

fn production_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
    {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            production_sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && !path.file_name().is_some_and(|name| name == "tests.rs")
        {
            found.push(path);
        }
    }
}

#[test]
fn every_operation_of_the_first_run_flow_has_a_production_caller() {
    // Arrange
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");

    // Act
    let missing: Vec<&str> = CALLERS
        .iter()
        .filter(|(_, call, dir)| {
            let mut files = Vec::new();
            production_sources(&src.join(dir), &mut files);
            !files.iter().any(|file| {
                fs::read_to_string(file)
                    .unwrap_or_else(|error| panic!("read {}: {error}", file.display()))
                    .contains(call)
            })
        })
        .map(|(name, _, _)| *name)
        .collect();

    // Assert
    assert!(missing.is_empty(), "no production caller for {missing:?}");
}
