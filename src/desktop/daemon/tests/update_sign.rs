//! AC-5: the `update-sign` binary of the test build (pinned test keys A and R) signs a manifest the updater
//! accepts, and refuses on CI, without `--key`, and for key material offered any other way.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use sha2::{Digest, Sha256};
use sunnycloud_daemon::updater::manifest::{Version, platform_key};
use sunnycloud_daemon::updater::{Slot, VerifyContext, verify_manifest};
use tempfile::TempDir;

/// 2026-11-12T00:00:00Z.
const NOW_MICROS: i64 = 1_794_441_600_000_000;

fn seed_a() -> [u8; 32] {
    Sha256::digest("sunnycloud updater test key A").into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct Workspace {
    _dir: TempDir,
    manifest: PathBuf,
    key: PathBuf,
}

fn workspace() -> Workspace {
    let dir = TempDir::new().expect("temp dir");
    let manifest = dir.path().join("update-manifest.json");
    let body = format!(
        r#"{{"format":1,"version":"1.0.1","serial":7,"expires":"2026-12-12T00:00:00Z","platforms":{{"{}":{{"asset":"sunnycloud-desktop_1.0.1_amd64.deb","sha256":"{}","size":3}}}}}}"#,
        platform_key(),
        hex(&Sha256::digest(b"abc"))
    );
    fs::write(&manifest, body).expect("manifest");
    let key = dir.path().join("active.key");
    fs::write(&key, seed_a()).expect("key");
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).expect("chmod");
    Workspace {
        _dir: dir,
        manifest,
        key,
    }
}

/// The tool with a clean CI environment plus `env`.
fn update_sign(
    args: &[&Path],
    flags: &[&str],
    env: &[(&str, &str)],
    stdin: Option<&[u8]>,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_update-sign"));
    command.env_remove("CI").env_remove("GITHUB_ACTIONS");
    for (flag, value) in flags.iter().zip(args) {
        command.arg(flag).arg(value);
    }
    command.args(&flags[args.len()..]);
    command.envs(env.iter().copied());
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("spawn update-sign");
    let mut input = child.stdin.take().expect("stdin");
    input
        .write_all(stdin.unwrap_or_default())
        .expect("write stdin");
    drop(input);
    child.wait_with_output().expect("update-sign output")
}

fn sig_path(manifest: &Path) -> PathBuf {
    PathBuf::from(format!("{}.sig", manifest.display()))
}

fn assert_no_key_printed(output: &Output) {
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!printed.contains(&hex(&seed_a())), "{printed}");
}

#[test]
fn update_sign_signature_is_accepted_by_the_updater() {
    // Arrange
    let w = workspace();

    // Act
    let output = update_sign(
        &[&w.manifest, &w.key],
        &["--manifest", "--key", "--slot", "active"],
        &[],
        None,
    );

    // Assert
    assert!(output.status.success(), "{output:?}");
    assert_no_key_printed(&output);
    let manifest = fs::read(&w.manifest).expect("manifest");
    let signature = fs::read(sig_path(&w.manifest)).expect("signature file");
    let platform = platform_key();
    let context = VerifyContext {
        now_unix_micros: NOW_MICROS,
        accepted_serial: Some(6),
        installed: Version::parse("1.0.0").expect("version"),
        platform: &platform,
    };
    let verified = verify_manifest(&manifest, Some(&signature), &context).expect("accepted");
    assert_eq!((verified.slot, verified.serial), (Slot::Active, 7));
}

#[test]
fn update_sign_refuses_to_run_on_ci() {
    for variable in ["CI", "GITHUB_ACTIONS"] {
        // Arrange
        let w = workspace();

        // Act
        let output = update_sign(
            &[&w.manifest, &w.key],
            &["--manifest", "--key", "--slot", "active"],
            &[(variable, "true")],
            None,
        );

        // Assert
        assert_eq!(output.status.code(), Some(2), "{variable}: {output:?}");
        assert!(!sig_path(&w.manifest).exists(), "{variable}");
    }
}

#[test]
fn update_sign_refuses_a_key_given_through_the_environment() {
    // Arrange
    let w = workspace();
    let seed = hex(&seed_a());
    let env = [
        ("UPDATE_SIGN_KEY", seed.as_str()),
        ("SUNNYCLOUD_UPDATER_KEY", seed.as_str()),
        ("SUNNYCLOUD_UPDATER_PRIVATE_KEY", seed.as_str()),
    ];

    // Act
    let output = update_sign(
        &[&w.manifest],
        &["--manifest", "--slot", "active"],
        &env,
        None,
    );

    // Assert
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(!sig_path(&w.manifest).exists());
    assert_no_key_printed(&output);
}

#[test]
fn update_sign_refuses_a_key_on_standard_input_or_as_an_argument() {
    // Arrange
    let w = workspace();
    let seed = seed_a();
    let stdin_path = Path::new("/dev/stdin");
    let seed_hex = hex(&seed);

    // Act
    let from_stdin = update_sign(
        &[&w.manifest, stdin_path],
        &["--manifest", "--key", "--slot", "active"],
        &[],
        Some(&seed),
    );
    let as_argument = update_sign(
        &[&w.manifest, Path::new(&seed_hex)],
        &["--manifest", "--key", "--slot", "active"],
        &[],
        None,
    );

    // Assert
    assert_eq!(from_stdin.status.code(), Some(1), "{from_stdin:?}");
    assert_eq!(as_argument.status.code(), Some(1), "{as_argument:?}");
    assert!(!sig_path(&w.manifest).exists());
    assert_no_key_printed(&from_stdin);
}
