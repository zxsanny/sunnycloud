//! IT-14-17 (AC-49): the two good builds of DS-31 are staged, the eight bad ones are refused one per check.

use tracing::Level;

use super::{
    EXPIRED, Harness, Release, TestKey, asset_name, harness, manifest, payload, signature_file,
};
/// WARN lines recorded after the first `from` events.
fn warns_since(h: &Harness, from: usize) -> Vec<String> {
    h.captured
        .events()
        .into_iter()
        .skip(from)
        .filter(|(level, _)| *level == Level::WARN)
        .map(|(_, line)| line)
        .collect()
}

async fn assert_good_build_staged(key: TestKey, slot: &str) {
    // Arrange
    let h = harness().await;
    h.publish(&Release::signed("1.0.1", 7, key));

    // Act
    let staged = h.updater.check_once().await.expect("good build accepted");

    // Assert
    assert_eq!(
        (staged.version.as_str(), staged.serial, staged.key.as_str()),
        ("1.0.1", 7, slot)
    );
    assert_eq!(std::fs::read(&staged.path).expect("staged file"), payload());
    assert_eq!(h.staging_files(), vec![asset_name("1.0.1")]);
    assert_eq!(h.accepted_serial(), Some(7));
    let view = h
        .updater
        .update_view()
        .expect("view")
        .expect("update ready");
    assert_eq!(view.version, "1.0.1");
    assert!(
        h.captured
            .events()
            .iter()
            .any(|(level, line)| *level == Level::INFO && line.contains("update staged")),
        "{:?}",
        h.captured.events()
    );
}

#[tokio::test]
async fn check_once_active_key_build_is_staged() {
    assert_good_build_staged(TestKey::A, "active").await;
}

#[tokio::test]
async fn check_once_recovery_key_build_is_staged() {
    assert_good_build_staged(TestKey::R, "recovery").await;
}

/// One bad build of DS-31: what the host serves and the check that must refuse it.
struct BadBuild {
    name: &'static str,
    release: Release,
    check: &'static str,
}

fn bad_builds() -> Vec<BadBuild> {
    let good = || Release::signed("1.0.1", 7, TestKey::A);
    let mut unsigned = good();
    unsigned.signature = None;
    let expired_manifest = manifest("1.0.1", 7, EXPIRED, &payload());
    let expired = Release {
        signature: Some(signature_file(TestKey::A, "active", &expired_manifest)),
        manifest: expired_manifest,
        ..good()
    };
    let mut altered = good();
    altered.asset[100] ^= 0x01;
    let mut truncated = good();
    truncated.asset.truncate(4096);
    vec![
        BadBuild {
            name: "unsigned",
            release: unsigned,
            check: "signature",
        },
        BadBuild {
            name: "unpinned key B",
            release: Release::signed("1.0.1", 7, TestKey::B),
            check: "signature",
        },
        BadBuild {
            name: "expired",
            release: expired,
            check: "expiry",
        },
        BadBuild {
            name: "replayed serial 7 after 7 was accepted",
            release: Release::signed("1.0.2", 7, TestKey::A),
            check: "replay",
        },
        BadBuild {
            name: "same version",
            release: Release::signed("1.0.0", 8, TestKey::A),
            check: "version",
        },
        BadBuild {
            name: "lower version",
            release: Release::signed("0.9.0", 8, TestKey::A),
            check: "version",
        },
        BadBuild {
            name: "altered payload",
            release: altered,
            check: "digest",
        },
        BadBuild {
            name: "truncated payload",
            release: truncated,
            check: "size",
        },
    ]
}

/// The replay case starts from a daemon that accepted serial 7 (v1.0.1); every other case from a fresh one.
async fn prepared(build: &BadBuild) -> Harness {
    let h = harness().await;
    if build.check == "replay" {
        h.publish(&Release::signed("1.0.1", 7, TestKey::A));
        h.updater.check_once().await.expect("serial 7 accepted");
    }
    h
}

#[tokio::test]
async fn check_once_eight_bad_builds_are_rejected_by_their_check() {
    let mut rejected = 0;
    for build in bad_builds() {
        // Arrange
        let h = prepared(&build).await;
        let staged_before = h.staging_files();
        let serial_before = h.accepted_serial();
        let view_before = h.updater.update_view().expect("view");
        h.publish(&build.release);
        let logged_before = h.captured.events().len();

        // Act
        let outcome = h.updater.check_once().await;

        // Assert
        let error = outcome.expect_err(build.name);
        assert_eq!(error.check(), build.check, "{}: {error}", build.name);
        assert_eq!(h.staging_files(), staged_before, "{}", build.name);
        assert_eq!(h.accepted_serial(), serial_before, "{}", build.name);
        assert_eq!(
            h.updater.update_view().expect("view"),
            view_before,
            "{}",
            build.name
        );
        let warn = warns_since(&h, logged_before);
        assert_eq!(warn.len(), 1, "{}: {warn:?}", build.name);
        assert!(
            warn[0].contains(&format!("check=\"{}\"", build.check)),
            "{}: {warn:?}",
            build.name
        );
        rejected += 1;
    }
    assert_eq!(rejected, 8);
}
