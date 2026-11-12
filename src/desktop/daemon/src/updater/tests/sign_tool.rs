//! The library half of `update-sign`: arguments, the CI guard, key-file rules and verify-before-write.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::{EXPIRES_IN_30_DAYS, TestKey, manifest, payload, seed};
use crate::updater::sign::{SignArgs, SignError, parse_args, refuse_on_ci, sign, signature_path};
use crate::updater::{Slot, verify_signature};

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

/// A manifest and a key file holding `key`'s seed with `mode`.
fn workspace(key: TestKey, mode: u32) -> (TempDir, PathBuf, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let manifest_path = dir.path().join("update-manifest.json");
    fs::write(
        &manifest_path,
        manifest("1.0.1", 7, EXPIRES_IN_30_DAYS, &payload()),
    )
    .expect("manifest");
    let key_path = dir.path().join("active.key");
    write_key(&key_path, &seed(key), mode);
    (dir, manifest_path, key_path)
}

#[cfg(unix)]
fn write_key(path: &Path, bytes: &[u8], mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, bytes).expect("key file");
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("chmod");
}

#[cfg(not(unix))]
fn write_key(path: &Path, bytes: &[u8], _mode: u32) {
    fs::write(path, bytes).expect("key file");
}

#[test]
fn parse_args_reads_the_three_options() {
    // Act
    let parsed = parse_args(args(&[
        "--slot",
        "recovery",
        "--manifest",
        "m.json",
        "--key",
        "k",
    ]));

    // Assert
    assert_eq!(
        parsed.expect("parsed"),
        SignArgs {
            manifest: "m.json".into(),
            key: "k".into(),
            slot: Slot::Recovery
        }
    );
}

#[test]
fn parse_args_refuses_anything_else() {
    // Arrange
    let refused = [
        args(&["--manifest", "m.json", "--slot", "active"]),
        args(&["--manifest", "m.json", "--key", "k", "--slot", "backup"]),
        args(&[
            "--manifest",
            "m.json",
            "--key",
            "k",
            "--key",
            "k2",
            "--slot",
            "active",
        ]),
        args(&[
            "--manifest",
            "m.json",
            "--key-hex",
            "00",
            "--slot",
            "active",
        ]),
        args(&["--manifest", "m.json", "--slot", "active", "--key"]),
    ];

    for list in refused {
        // Act
        let outcome = parse_args(list.clone());

        // Assert
        assert!(
            matches!(outcome, Err(SignError::Usage(_))),
            "{list:?}: {outcome:?}"
        );
    }
}

#[test]
fn refuse_on_ci_refuses_when_ci_or_github_actions_is_set() {
    // Arrange
    let only = |set: &'static str| move |name: &str| (name == set).then(|| OsString::from(""));

    // Act
    let outcomes = (
        refuse_on_ci(only("CI")),
        refuse_on_ci(only("GITHUB_ACTIONS")),
        refuse_on_ci(only("HOME")),
    );

    // Assert
    assert!(matches!(outcomes.0, Err(SignError::OnCi("CI"))));
    assert!(matches!(outcomes.1, Err(SignError::OnCi("GITHUB_ACTIONS"))));
    assert!(outcomes.2.is_ok());
}

#[test]
fn sign_writes_a_signature_the_daemon_accepts() {
    for (key, slot) in [(TestKey::A, Slot::Active), (TestKey::R, Slot::Recovery)] {
        // Arrange
        let (_dir, manifest_path, key_path) = workspace(key, 0o600);

        // Act
        let written = sign(&SignArgs {
            manifest: manifest_path.clone(),
            key: key_path,
            slot,
        })
        .expect("signed");

        // Assert
        assert_eq!(written, signature_path(&manifest_path));
        let manifest_bytes = fs::read(&manifest_path).expect("manifest");
        let signature = fs::read(&written).expect("sig");
        assert_eq!(verify_signature(&manifest_bytes, &signature), Ok(slot));
    }
}

#[test]
fn sign_refuses_a_key_that_does_not_match_the_slot() {
    // Arrange
    let (_dir, manifest_path, key_path) = workspace(TestKey::B, 0o600);

    // Act
    let outcome = sign(&SignArgs {
        manifest: manifest_path.clone(),
        key: key_path,
        slot: Slot::Active,
    });

    // Assert
    assert!(
        matches!(outcome, Err(SignError::WrongKey("active"))),
        "{outcome:?}"
    );
    assert!(!signature_path(&manifest_path).exists());
}

#[cfg(unix)]
#[test]
fn sign_refuses_a_key_file_others_can_read_or_a_symlink() {
    // Arrange
    let (dir, manifest_path, key_path) = workspace(TestKey::A, 0o640);
    let link = dir.path().join("link.key");
    std::os::unix::fs::symlink(&key_path, &link).expect("symlink");

    // Act
    let group_readable = sign(&SignArgs {
        manifest: manifest_path.clone(),
        key: key_path,
        slot: Slot::Active,
    });
    let symlink = sign(&SignArgs {
        manifest: manifest_path.clone(),
        key: link,
        slot: Slot::Active,
    });

    // Assert
    assert!(
        matches!(group_readable, Err(SignError::KeyFilePermissions)),
        "{group_readable:?}"
    );
    assert!(
        matches!(symlink, Err(SignError::KeyFilePermissions)),
        "{symlink:?}"
    );
    assert!(!signature_path(&manifest_path).exists());
}

#[test]
fn sign_refuses_a_short_key_file_and_a_malformed_manifest() {
    // Arrange
    let (dir, manifest_path, key_path) = workspace(TestKey::A, 0o600);
    let short_key = dir.path().join("short.key");
    write_key(&short_key, &seed(TestKey::A)[..31], 0o600);
    let bad_manifest = dir.path().join("bad.json");
    fs::write(&bad_manifest, br#"{"format": 2}"#).expect("bad manifest");

    // Act
    let short = sign(&SignArgs {
        manifest: manifest_path,
        key: short_key,
        slot: Slot::Active,
    });
    let malformed = sign(&SignArgs {
        manifest: bad_manifest,
        key: key_path,
        slot: Slot::Active,
    });

    // Assert
    assert!(matches!(short, Err(SignError::KeyFileLength)), "{short:?}");
    assert!(
        matches!(malformed, Err(SignError::Manifest)),
        "{malformed:?}"
    );
}
