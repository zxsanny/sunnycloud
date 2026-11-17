//! AZ-3807 AC-3 against the real binary: `doctor --pre-flight` runs the pre-flight checks without dialling the
//! database, which here is a listener nobody should reach.

mod config_support;

use std::fs;
use std::net::TcpListener;
use std::process::{Command, Stdio};

use config_support::Setup;

#[test]
fn pre_flight_lists_the_key_and_volume_checks_and_never_dials_the_database() {
    // Arrange: a database port that records any connection, a blob volume and a valid 32-byte key file.
    let database = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
    database
        .set_nonblocking(true)
        .expect("the listener can be polled");
    let port = database.local_addr().expect("bound address").port();
    let mut setup = Setup::valid(port);
    let volume = setup.dir.join("blobs");
    fs::create_dir_all(&volume).expect("blob volume");
    let key = setup.dir.join("master.key");
    fs::write(&key, [0x5au8; 32]).expect("key file");
    setup
        .set("blob_root", &volume.display().to_string())
        .secret("master_key_file", &key.display().to_string());
    // The one provider is a local OIDC issuer nobody answers for, so the test needs no network.
    setup
        .remove("github_client_id")
        .without_secret("provider_github_client_secret");
    setup
        .set("oidc_issuer", "http://127.0.0.1:1")
        .set("oidc_client_id", "client")
        .set("allow_plain_http_dev", "true")
        .secret("provider_oidc_client_secret", "oidc-secret");

    // Act
    let output = Command::new(env!("CARGO_BIN_EXE_sunnycloud"))
        .args(["doctor", "--pre-flight"])
        .env_clear()
        .envs(&setup.env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("the binary runs");

    // Assert
    let stdout = String::from_utf8(output.stdout).expect("UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("UTF-8");
    assert!(database.accept().is_err(), "the database was dialled");
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "{:?}: {stderr}",
        output.status.code()
    );
    assert!(stdout.contains("check: master_key"), "{stdout}");
    assert!(stdout.contains("check: blob_volume"), "{stdout}");
    assert!(stdout.contains("check: identity_providers"), "{stdout}");
    assert!(
        stdout.contains("https://cloud.example.org/api/v1/auth/providers/oidc/callback"),
        "{stdout}"
    );
    assert!(!stdout.contains("check: database"), "{stdout}");
    assert!(!stdout.contains("check: identity_setup"), "{stdout}");
    assert!(!stdout.contains("media_worker"), "{stdout}");
    assert!(!stdout.contains("blob_files_missing"), "{stdout}");
}
