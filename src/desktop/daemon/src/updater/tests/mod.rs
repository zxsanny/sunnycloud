//! DS-31 fixtures made in the tests: keys A (active), R (recovery) and B (unpinned) are derived from public
//! labels, so no real updater key exists anywhere. The updater runs in-process against [`host::ReleaseHost`].

mod attacks;
mod host;
mod matrix;
mod redirects;
mod sign_tool;
mod verify_unit;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use sc_clock::{ManualClock, Timestamp};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::manifest::{MANIFEST_NAME, SIGNATURE_NAME, Version, platform_key};
use super::{Build, RedirectRule, STAGING_DIR_NAME, Updater, UpdaterConfig};
use crate::ipc::tests::capture::Captured;
use crate::local_index::LocalIndexStore;
use host::{ReleaseHost, Reply};

/// 2026-11-12T00:00:00Z, the test clock's start.
pub(crate) const NOW_SECS: i64 = 1_794_441_600;
/// `NOW` plus 30 days.
pub(crate) const EXPIRES_IN_30_DAYS: &str = "2026-12-12T00:00:00Z";
pub(crate) const EXPIRED: &str = "2026-11-01T00:00:00Z";
pub(crate) const INSTALLED: &str = "1.0.0";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TestKey {
    A,
    R,
    B,
}

pub(crate) fn seed(key: TestKey) -> [u8; 32] {
    Sha256::digest(format!("sunnycloud updater test key {key:?}")).into()
}

pub(crate) fn signing_key(key: TestKey) -> SigningKey {
    SigningKey::from_bytes(&seed(key))
}

pub(crate) fn payload() -> Vec<u8> {
    (0..8192u32).map(|i| (i % 251) as u8).collect()
}

pub(crate) fn asset_name(version: &str) -> String {
    format!("sunnycloud-desktop_{version}_test.pkg")
}

/// A format-1 manifest whose entry for this machine describes `body`.
pub(crate) fn manifest(version: &str, serial: u64, expires: &str, body: &[u8]) -> Vec<u8> {
    let entry = json!({
        "asset": asset_name(version),
        "sha256": hex(&Sha256::digest(body)),
        "size": body.len(),
    });
    let document = json!({
        "format": 1,
        "version": version,
        "serial": serial,
        "expires": expires,
        "platforms": { platform_key(): entry },
    });
    serde_json::to_vec_pretty(&document).expect("manifest json")
}

/// `{"key": hint, "sig": base64}` with the signature of `key` over the exact bytes.
pub(crate) fn signature_file(key: TestKey, hint: &str, manifest: &[u8]) -> Vec<u8> {
    let signature = signing_key(key).sign(manifest);
    serde_json::to_vec(&json!({ "key": hint, "sig": STANDARD.encode(signature.to_bytes()) }))
        .expect("sig json")
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// One published release on the host.
pub(crate) struct Release {
    pub(crate) version: &'static str,
    pub(crate) manifest: Vec<u8>,
    pub(crate) signature: Option<Vec<u8>>,
    pub(crate) asset: Vec<u8>,
}

impl Release {
    /// Case 1 of DS-31 with any version, serial and key: the asset matches the manifest.
    pub(crate) fn signed(version: &'static str, serial: u64, key: TestKey) -> Self {
        let manifest = manifest(version, serial, EXPIRES_IN_30_DAYS, &payload());
        let hint = if key == TestKey::R {
            "recovery"
        } else {
            "active"
        };
        Self {
            version,
            signature: Some(signature_file(key, hint, &manifest)),
            manifest,
            asset: payload(),
        }
    }
}

pub(crate) struct Harness {
    pub(crate) dir: TempDir,
    pub(crate) clock: Arc<ManualClock>,
    pub(crate) host: ReleaseHost,
    /// A listed redirect host, standing in for GitHub's release-asset host.
    pub(crate) cdn: ReleaseHost,
    pub(crate) updater: Arc<Updater>,
    /// Every event of this test's thread. Each harness keeps a collector installed for its whole life, so
    /// the first registration of an updater callsite, on whichever test thread, never caches "no interest".
    pub(crate) captured: Captured,
    _trace: tracing::subscriber::DefaultGuard,
}

pub(crate) const TEST_CONFIG: UpdaterConfig = UpdaterConfig {
    check_interval: Duration::from_millis(400),
    connect_timeout: Duration::from_secs(5),
    request_timeout: Duration::from_secs(10),
};

pub(crate) async fn harness() -> Harness {
    let captured = Captured::default();
    let trace = captured.install();
    tracing::callsite::rebuild_interest_cache();
    let dir = TempDir::new().expect("temp dir");
    let index = Arc::new(LocalIndexStore::open(dir.path()).expect("index"));
    let clock = Arc::new(ManualClock::new(Timestamp::from_unix_micros(
        NOW_SECS * 1_000_000,
    )));
    let host = ReleaseHost::start().await;
    let cdn = ReleaseHost::start().await;
    let build = Build {
        source: host.source(),
        redirects: RedirectRule {
            hosts: vec![host.allowed(), cdn.allowed()],
            require_https: false,
        },
        installed: Version::parse(INSTALLED).expect("version"),
    };
    let updater = Updater::for_test_build(
        build,
        Arc::clone(&index),
        dir.path(),
        Arc::clone(&clock) as Arc<dyn sc_clock::Clock>,
        TEST_CONFIG,
    )
    .expect("updater");
    Harness {
        dir,
        clock,
        host,
        cdn,
        updater: Arc::new(updater),
        captured,
        _trace: trace,
    }
}

impl Harness {
    pub(crate) fn publish(&self, release: &Release) {
        self.host
            .set_latest(MANIFEST_NAME, Some(Reply::Body(release.manifest.clone())));
        self.host
            .set_latest(SIGNATURE_NAME, release.signature.clone().map(Reply::Body));
        self.host.set_asset(
            &format!("v{}", release.version),
            &asset_name(release.version),
            release.asset.clone(),
        );
    }

    /// Every file in the staging directory, partial files included.
    pub(crate) fn staging_files(&self) -> Vec<String> {
        let dir: PathBuf = self.dir.path().join(STAGING_DIR_NAME);
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    pub(crate) fn accepted_serial(&self) -> Option<u64> {
        self.updater.accepted_serial().expect("serial")
    }
}
