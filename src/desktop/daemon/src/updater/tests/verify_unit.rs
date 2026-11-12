//! Each manifest check in isolation, the key hint, and the strict parsers.

use serde_json::json;

use super::{
    EXPIRED, EXPIRES_IN_30_DAYS, INSTALLED, NOW_SECS, TestKey, manifest, payload, seed,
    signature_file, signing_key,
};
use crate::updater::manifest::{
    Version, is_plain_asset_name, parse_expires, parse_sha256, platform_key,
};
use crate::updater::{Rejection, Slot, VerifyContext, pinned_key, verify_manifest};

fn context(platform: &str, accepted_serial: Option<u64>) -> VerifyContext<'_> {
    VerifyContext {
        now_unix_micros: NOW_SECS * 1_000_000,
        accepted_serial,
        installed: Version::parse(INSTALLED).expect("version"),
        platform,
    }
}

fn check(
    manifest: &[u8],
    signature: Option<&[u8]>,
    accepted: Option<u64>,
) -> Result<Slot, Rejection> {
    let platform = platform_key();
    verify_manifest(manifest, signature, &context(&platform, accepted)).map(|v| v.slot)
}

fn good() -> Vec<u8> {
    manifest("1.0.1", 7, EXPIRES_IN_30_DAYS, &payload())
}

#[test]
fn pinned_keys_are_the_public_halves_of_test_keys_a_and_r() {
    // Arrange
    let active = signing_key(TestKey::A).verifying_key();
    let recovery = signing_key(TestKey::R).verifying_key();

    // Act
    let pinned = (pinned_key(Slot::Active), pinned_key(Slot::Recovery));

    // Assert
    assert_eq!(pinned, (Some(active), Some(recovery)));
    assert_ne!(seed(TestKey::B), seed(TestKey::A));
}

#[test]
fn verify_manifest_active_and_recovery_signatures_are_accepted() {
    // Arrange
    let m = good();

    // Act
    let active = check(&m, Some(&signature_file(TestKey::A, "active", &m)), None);
    let recovery = check(&m, Some(&signature_file(TestKey::R, "recovery", &m)), None);

    // Assert
    assert_eq!((active, recovery), (Ok(Slot::Active), Ok(Slot::Recovery)));
}

#[test]
fn verify_manifest_key_hint_is_only_a_hint() {
    // Arrange
    let m = good();

    // Act
    let wrong_hint = check(&m, Some(&signature_file(TestKey::R, "active", &m)), None);
    let junk_hint = check(
        &m,
        Some(&signature_file(TestKey::A, "https://k.example/key", &m)),
        None,
    );

    // Assert
    assert_eq!(
        (wrong_hint, junk_hint),
        (Ok(Slot::Recovery), Ok(Slot::Active))
    );
}

#[test]
fn verify_manifest_each_failure_names_its_check() {
    // Arrange
    let m = good();
    let a = |bytes: &[u8]| signature_file(TestKey::A, "active", bytes);
    let expired = manifest("1.0.1", 7, EXPIRED, &payload());
    let same = manifest(INSTALLED, 8, EXPIRES_IN_30_DAYS, &payload());
    let lower = manifest("0.9.9", 8, EXPIRES_IN_30_DAYS, &payload());
    let mut flipped = a(&m);
    let mut tampered = m.clone();
    tampered[10] ^= 0x01;
    flipped.truncate(flipped.len() - 1);
    let cases: Vec<(&str, Result<Slot, Rejection>, &str)> = vec![
        ("unsigned", check(&m, None, None), "signature"),
        (
            "unpinned",
            check(&m, Some(&signature_file(TestKey::B, "active", &m)), None),
            "signature",
        ),
        (
            "bytes changed",
            check(&tampered, Some(&a(&m)), None),
            "signature",
        ),
        ("bad sig file", check(&m, Some(&flipped), None), "signature"),
        (
            "expired",
            check(&expired, Some(&a(&expired)), None),
            "expiry",
        ),
        ("replay equal", check(&m, Some(&a(&m)), Some(7)), "replay"),
        ("replay lower", check(&m, Some(&a(&m)), Some(9)), "replay"),
        (
            "same version",
            check(&same, Some(&a(&same)), None),
            "version",
        ),
        (
            "lower version",
            check(&lower, Some(&a(&lower)), None),
            "version",
        ),
    ];

    for (name, outcome, expected) in cases {
        // Act
        let error = outcome.expect_err(name);

        // Assert
        assert_eq!(error.check(), expected, "{name}: {error}");
    }
}

fn signed_document(document: serde_json::Value) -> Result<Slot, Rejection> {
    let bytes = serde_json::to_vec(&document).expect("json");
    let signature = signature_file(TestKey::A, "active", &bytes);
    check(&bytes, Some(&signature), None)
}

fn document(asset: &str, platform: &str) -> serde_json::Value {
    json!({
        "format": 1, "version": "1.0.1", "serial": 7, "expires": EXPIRES_IN_30_DAYS,
        "platforms": { platform: { "asset": asset, "sha256": "ab".repeat(32), "size": 10 } },
    })
}

#[test]
fn verify_manifest_layout_platform_and_source_checks() {
    // Arrange
    let mut format_2 = document("a.deb", &platform_key());
    format_2["format"] = json!(2);
    let mut bad_expires = document("a.deb", &platform_key());
    bad_expires["expires"] = json!("2026-12-12");
    let cases = [
        ("format 2", signed_document(format_2), "layout"),
        (
            "expires not RFC 3339 UTC",
            signed_document(bad_expires),
            "layout",
        ),
        (
            "no entry",
            signed_document(document("a.deb", "plan9-mips")),
            "platform",
        ),
        (
            "url asset",
            signed_document(document("https://evil.example/a.deb", &platform_key())),
            "source",
        ),
        (
            "traversal",
            signed_document(document("../a.deb", &platform_key())),
            "source",
        ),
    ];

    for (name, outcome, expected) in cases {
        // Act
        let error = outcome.expect_err(name);

        // Assert
        assert_eq!(error.check(), expected, "{name}: {error}");
    }
}

#[test]
fn version_parse_accepts_only_three_decimal_parts() {
    // Arrange
    let inputs = [
        "1.0.1",
        "10.20.30",
        "1.0",
        "1.0.1.2",
        "1.0.1-rc1",
        "v1.0.1",
        "1..1",
        "+1.0.0",
    ];

    // Act
    let parsed: Vec<bool> = inputs
        .iter()
        .map(|text| Version::parse(text).is_some())
        .collect();

    // Assert
    assert_eq!(
        parsed,
        [true, true, false, false, false, false, false, false]
    );
    assert!(Version::parse("1.0.10") > Version::parse("1.0.9"));
}

#[test]
fn parse_expires_reads_utc_seconds_and_refuses_other_shapes() {
    // Arrange
    let valid = [
        ("1970-01-01T00:00:00Z", 0),
        ("2000-02-29T00:00:00Z", 951_782_400),
        ("2027-01-15T00:00:00Z", 1_799_971_200),
    ];
    let invalid = [
        "2001-02-29T00:00:00Z",
        "2026-11-12T00:00:00+00:00",
        "2026-11-12T24:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-11-12t00:00:00Z",
    ];

    // Act
    let read: Vec<Option<i64>> = valid.iter().map(|(text, _)| parse_expires(text)).collect();
    let refused = invalid.iter().all(|text| parse_expires(text).is_none());

    // Assert
    let expected: Vec<Option<i64>> = valid
        .iter()
        .map(|(_, secs)| Some(secs * 1_000_000))
        .collect();
    assert_eq!(read, expected);
    assert!(refused);
}

#[test]
fn asset_and_digest_parsers_are_strict() {
    // Arrange / Act
    let names = [
        "sunnycloud-desktop_1.0.1_amd64.deb",
        ".hidden",
        "a/b",
        "a\\b",
        "",
        "a b",
    ]
    .map(is_plain_asset_name);
    let digests = ["AB".repeat(32), "ab".repeat(31), "zz".repeat(32)]
        .map(|text| parse_sha256(&text).is_some());

    // Assert
    assert_eq!(names, [true, false, false, false, false, false]);
    assert_eq!(digests, [true, false, false]);
}
