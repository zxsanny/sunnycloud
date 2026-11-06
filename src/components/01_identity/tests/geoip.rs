//! AC-7: the production GeoIP reader over a MaxMind database file, and its use by the approval view. The
//! database is written at test time (`support::geoip`) and read back by the real `maxminddb` reader; the
//! approval test runs on real PostgreSQL.

mod support;

use std::sync::Arc;

use contracts::DeviceGrantService;
use sqlx::Connection;
use sunnycloud_identity::{GeoLookup, MaxmindGeoLookup};
use support::Harness;
use support::device::{grants, ip, member};
use support::geoip::write_fixture;

fn open(path: &std::path::Path) -> MaxmindGeoLookup {
    MaxmindGeoLookup::open(Some(path.to_str().expect("utf-8 temp path")))
}

#[test]
fn a_known_address_gives_city_and_country_for_both_ip_versions() {
    // Arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let reader = open(&write_fixture(dir.path()));

    // Act
    let kyiv = reader.locate(ip("203.0.113.7"));
    let lviv = reader.locate(ip("198.51.100.9"));
    let berlin = reader.locate(ip("2001:db8::1"));
    let mapped = reader.locate(ip("::ffff:203.0.113.7"));

    // Assert
    assert_eq!(kyiv.as_deref(), Some("Kyiv, Ukraine"));
    assert_eq!(lviv.as_deref(), Some("Lviv, Ukraine"));
    assert_eq!(berlin.as_deref(), Some("Berlin, Germany"));
    assert_eq!(mapped.as_deref(), Some("Kyiv, Ukraine"));
}

#[test]
fn a_country_only_record_gives_the_country_and_a_city_only_record_gives_nothing() {
    // Arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let reader = open(&write_fixture(dir.path()));

    // Act
    let country_only = reader.locate(ip("192.0.2.10"));
    let city_only = reader.locate(ip("203.0.114.10"));

    // Assert
    assert_eq!(country_only.as_deref(), Some("Poland"));
    assert_eq!(city_only, None);
}

#[test]
fn an_address_missing_from_the_file_is_unknown() {
    // Arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let reader = open(&write_fixture(dir.path()));

    // Act
    let v4 = reader.locate(ip("8.8.8.8"));
    let v6 = reader.locate(ip("2001:4860::8888"));

    // Assert
    assert_eq!(v4, None);
    assert_eq!(v6, None);
}

#[test]
fn private_loopback_and_link_local_addresses_are_unknown_even_when_the_file_has_a_record() {
    // Arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let reader = open(&write_fixture(dir.path()));
    let local = ["10.0.0.5", "127.0.0.1", "fd00::1", "::1", "169.254.1.1"];

    // Act
    let places: Vec<Option<String>> = local.iter().map(|text| reader.locate(ip(text))).collect();

    // Assert
    assert_eq!(places, vec![None; local.len()]);
}

#[test]
fn a_missing_path_or_file_leaves_every_location_unknown() {
    // Arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let absent = dir.path().join("absent.mmdb");

    // Act
    let unconfigured = MaxmindGeoLookup::open(None);
    let missing = open(&absent);

    // Assert
    assert_eq!(unconfigured.locate(ip("203.0.113.7")), None);
    assert_eq!(missing.locate(ip("203.0.113.7")), None);
}

#[test]
fn a_garbage_file_leaves_every_location_unknown() {
    // Arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("garbage.mmdb");
    std::fs::write(&path, b"this is not a MaxMind database").expect("write garbage");

    // Act
    let reader = open(&path);

    // Assert
    assert_eq!(reader.locate(ip("203.0.113.7")), None);
}

#[tokio::test]
async fn the_approval_view_shows_both_locations_from_the_file_and_flags_the_mismatch() {
    // Arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let approver = member(&mut conn).await;
    let service = grants(&mut harness, Arc::new(open(&write_fixture(dir.path()))));
    let authorization = service
        .request("lab-1", "linux", ip("203.0.113.7"))
        .await
        .expect("request");

    // Act
    let view = service
        .lookup_for_approval(&approver, &authorization.user_code, ip("198.51.100.9"))
        .await
        .expect("lookup");

    // Assert
    assert_eq!(view.requester_location.as_deref(), Some("Kyiv, Ukraine"));
    assert_eq!(view.approver_location.as_deref(), Some("Lviv, Ukraine"));
    assert!(view.mismatch);
    conn.close().await.expect("close fixture connection");
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}
