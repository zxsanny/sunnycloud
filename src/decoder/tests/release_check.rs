//! AC-4 of AZ-3856: the check that a decoder-runner binary carries no freeze trigger. The binary
//! under test is the one cargo built for this run, so the suite proves the check twice: a run
//! without the `test-freeze` feature must pass it, a run with the feature must fail it.
use std::path::Path;
use std::process::{Command, Output};

fn check(binary: &Path) -> Output {
    Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/check-no-test-freeze.sh"
        ))
        .arg(binary)
        .output()
        .expect("run the check script")
}

#[test]
fn ac4_the_check_passes_without_the_feature_and_fails_with_it() {
    let binary = Path::new(env!("CARGO_BIN_EXE_decoder-runner"));

    let output = check(binary);

    let report = String::from_utf8_lossy(&output.stdout);
    if cfg!(feature = "test-freeze") {
        assert_eq!(output.status.code(), Some(1), "{report}");
        assert!(report.contains("test_freeze_main_loop"), "{report}");
    } else {
        assert_eq!(output.status.code(), Some(0), "{report}");
    }
}

#[test]
fn a_missing_binary_is_a_tool_error_not_a_pass() {
    let output = check(Path::new("/nonexistent/decoder-runner"));

    assert_eq!(output.status.code(), Some(2));
}
