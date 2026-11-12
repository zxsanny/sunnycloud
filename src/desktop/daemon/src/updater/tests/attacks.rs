//! ST-14-3 (downgrade and key confusion), AC-3 (fixed source, overrides ignored) and the periodic check.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sc_config::{ClientSettings, ConfigSources, Environment, load};
use serde_json::json;
use sha2::{Digest, Sha256};

use super::host::{ReleaseHost, Reply};
use super::{
    EXPIRES_IN_30_DAYS, Release, TEST_CONFIG, TestKey, asset_name, harness, hex, manifest, payload,
    signature_file, signing_key,
};
use crate::updater::manifest::{MANIFEST_NAME, SIGNATURE_NAME, platform_key};

#[tokio::test]
async fn check_once_replayed_older_manifest_is_refused_and_serial_kept() {
    // Arrange
    let h = harness().await;
    h.publish(&Release::signed("1.0.2", 8, TestKey::A));
    h.updater.check_once().await.expect("serial 8 accepted");
    h.publish(&Release::signed("1.0.1", 7, TestKey::A));

    // Act
    let error = h.updater.check_once().await.expect_err("replay refused");

    // Assert
    assert_eq!(error.check(), "replay");
    assert_eq!(h.accepted_serial(), Some(8));
    assert_eq!(h.staging_files(), vec![asset_name("1.0.2")]);
}

#[tokio::test]
async fn check_once_recovery_key_lower_version_is_refused() {
    // Arrange
    let h = harness().await;
    h.publish(&Release::signed("0.9.0", 9, TestKey::R));

    // Act
    let error = h.updater.check_once().await.expect_err("downgrade refused");

    // Assert
    assert_eq!(error.check(), "version");
    assert_eq!(h.accepted_serial(), None);
    assert!(h.staging_files().is_empty());
}

/// The attacker lists key B in the manifest and in the signature file's `key` field and signs with B.
#[tokio::test]
async fn check_once_key_listed_in_manifest_is_never_used() {
    // Arrange
    let h = harness().await;
    let b_public = STANDARD.encode(signing_key(TestKey::B).verifying_key().to_bytes());
    let body = payload();
    let document = json!({
        "format": 1, "version": "1.0.1", "serial": 7, "expires": EXPIRES_IN_30_DAYS,
        "public_key": b_public,
        "platforms": { platform_key(): {
            "asset": asset_name("1.0.1"),
            "sha256": hex(&Sha256::digest(&body)),
            "size": body.len() } },
    });
    let manifest_bytes = serde_json::to_vec(&document).expect("json");
    let release = Release {
        version: "1.0.1",
        signature: Some(signature_file(TestKey::B, &b_public, &manifest_bytes)),
        manifest: manifest_bytes,
        asset: body,
    };
    h.publish(&release);

    // Act
    let error = h.updater.check_once().await.expect_err("refused");

    // Assert
    assert_eq!(error.check(), "signature");
    assert!(h.staging_files().is_empty());
    assert_eq!(h.accepted_serial(), None);
    let fetched = h.host.requests();
    assert_eq!(fetched.len(), 2, "only manifest and signature: {fetched:?}");
}

/// The same manifest with an embedded key, validly signed by A, is refused as an unknown layout.
#[tokio::test]
async fn check_once_active_signed_manifest_with_embedded_key_is_refused() {
    // Arrange
    let h = harness().await;
    let mut document: serde_json::Value =
        serde_json::from_slice(&manifest("1.0.1", 7, EXPIRES_IN_30_DAYS, &payload()))
            .expect("json");
    document["public_key"] = json!("AAAA");
    let manifest_bytes = serde_json::to_vec(&document).expect("json");
    h.publish(&Release {
        version: "1.0.1",
        signature: Some(signature_file(TestKey::A, "active", &manifest_bytes)),
        manifest: manifest_bytes,
        asset: payload(),
    });

    // Act
    let error = h.updater.check_once().await.expect_err("refused");

    // Assert
    assert_eq!(error.check(), "layout");
    assert!(h.staging_files().is_empty());
}

/// `SUNNYCLOUD_*` variables and a settings file point the updater keys and source at a hostile host that
/// also plays the sync server; the updater still contacts only its fixed host and trusts only A and R.
#[tokio::test]
async fn check_once_env_and_settings_overrides_change_nothing() {
    // Arrange
    let h = harness().await;
    let hostile = ReleaseHost::start().await;
    let b_hex = hex(&signing_key(TestKey::B).verifying_key().to_bytes());
    let settings_file = h.dir.path().join("settings.json");
    let overrides = json!({
        "updater_source": hostile.source(),
        "updater_public_key_1": b_hex,
        "updater_public_key_2": b_hex,
        "sync_server_url": hostile.source(),
    });
    std::fs::write(&settings_file, overrides.to_string()).expect("settings file");
    let env: BTreeMap<String, String> = [
        ("SUNNYCLOUD_UPDATER_SOURCE", hostile.source()),
        ("SUNNYCLOUD_UPDATER_PUBLIC_KEY_1", b_hex.clone()),
        ("SUNNYCLOUD_SYNC_SERVER_URL", hostile.source()),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
    let sources = ConfigSources {
        env,
        files: vec![settings_file],
        environment: Environment::Development,
        ..ConfigSources::default()
    };
    let settings = load::<ClientSettings>(&sources).expect("client settings bind");
    assert_eq!(
        settings.updater_source.as_deref(),
        Some(hostile.source().as_str())
    );
    let hostile_release = Release::signed("1.0.1", 7, TestKey::B);
    hostile.set_latest(
        MANIFEST_NAME,
        Some(Reply::Body(hostile_release.manifest.clone())),
    );
    hostile.set_latest(SIGNATURE_NAME, hostile_release.signature.map(Reply::Body));
    h.publish(&Release::signed("1.0.1", 7, TestKey::B));

    // Act
    let b_signed = h.updater.check_once().await;
    h.publish(&Release::signed("1.0.1", 7, TestKey::A));
    let a_signed = h.updater.check_once().await;

    // Assert
    assert_eq!(b_signed.expect_err("B still unpinned").check(), "signature");
    assert_eq!(a_signed.expect("fixed host build staged").version, "1.0.1");
    assert!(hostile.requests().is_empty(), "{:?}", hostile.requests());
    assert!(!h.host.requests().is_empty());
}

#[tokio::test]
async fn run_checks_again_after_the_interval_and_stops_on_shutdown() {
    // Arrange
    let h = harness().await;
    h.publish(&Release::signed("1.0.1", 7, TestKey::A));
    let updater = Arc::clone(&h.updater);
    let task = tokio::spawn(async move { updater.run().await });
    wait_for_serial(&h, 7, || {}).await;
    h.publish(&Release::signed("1.0.2", 8, TestKey::A));

    // Act
    wait_for_serial(&h, 8, || h.clock.tick(TEST_CONFIG.check_interval)).await;
    h.clock.shutdown();

    // Assert
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("run returns after shutdown")
        .expect("run task");
    assert_eq!(h.staging_files(), vec![asset_name("1.0.2")]);
}

/// Polls until the stored serial is `serial`, calling `nudge` between polls; fails after 10 s.
async fn wait_for_serial(h: &super::Harness, serial: u64, nudge: impl Fn()) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.accepted_serial() != Some(serial) {
        assert!(Instant::now() < deadline, "serial {serial} never stored");
        nudge();
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
