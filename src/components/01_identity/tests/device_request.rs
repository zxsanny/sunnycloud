//! AC-1 (IT-01-11 request part, FT-P-66): a device authorization is issued and stored by hash only. Real
//! PostgreSQL, manual clock.

mod support;

use std::collections::HashSet;
use std::time::Duration;

use contracts::DeviceGrantService;
use sc_ids::sha256_token;
use sqlx::Connection;
use support::device::{geo, grants, ip};
use support::{Harness, START};

const USER_CODE_ALPHABET: &str = "BCDFGHJKLMNPQRSTVWXZ";

#[tokio::test]
async fn request_returns_the_codes_and_stores_only_the_hash() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let service = grants(&mut harness, geo(&[("10.0.0.5", "Kyiv")]));

    // Act
    let authorization = service
        .request("lab-1", "linux", ip("10.0.0.5"))
        .await
        .expect("request");

    // Assert
    assert_eq!(authorization.device_code.expose().len(), 43, "256 bits");
    let user_code = authorization.user_code.as_str();
    assert_eq!(user_code.len(), 8);
    assert!(user_code.chars().all(|c| USER_CODE_ALPHABET.contains(c)));
    assert_eq!(authorization.interval, harness.config.device_poll_interval);
    assert_eq!(authorization.expires_in, harness.config.device_code_ttl);
    assert!(authorization.expires_in <= Duration::from_secs(600));
    let row: (
        Vec<u8>,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        bool,
    ) = sqlx::query_as(
        "SELECT device_code_hash, user_code, device_name, platform, host(requester_ip), \
             requester_location, status, account_id IS NULL FROM device_grants",
    )
    .fetch_one(&mut conn)
    .await
    .expect("the one grant row");
    assert_eq!(
        row.0,
        sha256_token(authorization.device_code.expose())
            .as_bytes()
            .to_vec()
    );
    assert_eq!(row.1, user_code);
    assert_eq!(
        (row.2.as_str(), row.3.as_str(), row.4.as_str()),
        ("lab-1", "linux", "10.0.0.5")
    );
    assert_eq!(row.5.as_deref(), Some("Kyiv"));
    assert_eq!(row.6, "pending");
    assert!(row.7, "a pending grant has no account");
    let stored: String = sqlx::query_scalar("SELECT g::text FROM device_grants g")
        .fetch_one(&mut conn)
        .await
        .expect("row as text");
    assert!(
        !stored.contains(authorization.device_code.expose()),
        "the device code must not be stored"
    );
    let window: i64 = sqlx::query_scalar(
        "SELECT (extract(epoch FROM expires_at - created_at) * 1000000)::bigint FROM device_grants",
    )
    .fetch_one(&mut conn)
    .await
    .expect("window");
    assert_eq!(
        u64::try_from(window).expect("positive window"),
        u64::try_from(harness.config.device_code_ttl.as_micros()).expect("fits")
    );
    let created: i64 = sqlx::query_scalar(
        "SELECT (extract(epoch FROM created_at) * 1000000)::bigint FROM device_grants",
    )
    .fetch_one(&mut conn)
    .await
    .expect("created");
    assert_eq!(created, START.as_unix_micros());
    conn.close().await.expect("close fixture connection");
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn request_from_an_unknown_address_stores_no_location() {
    // Arrange
    let mut harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let service = grants(&mut harness, geo(&[]));

    // Act
    service
        .request("lab-1", "windows", ip("2001:db8::7"))
        .await
        .expect("request");

    // Assert
    let row: (String, Option<String>) =
        sqlx::query_as("SELECT host(requester_ip), requester_location FROM device_grants")
            .fetch_one(&mut conn)
            .await
            .expect("the one grant row");
    assert_eq!(row, ("2001:db8::7".to_owned(), None));
    conn.close().await.expect("close fixture connection");
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}

#[tokio::test]
async fn requests_get_distinct_device_and_user_codes() {
    // Arrange
    let mut harness = Harness::new().await;
    let service = grants(&mut harness, geo(&[]));

    // Act
    let mut device_codes = HashSet::new();
    let mut user_codes = HashSet::new();
    for _ in 0..50 {
        let authorization = service
            .request("lab-1", "macos", ip("10.0.0.5"))
            .await
            .expect("request");
        device_codes.insert(authorization.device_code.expose().to_owned());
        user_codes.insert(authorization.user_code.as_str().to_owned());
    }

    // Assert
    assert_eq!(device_codes.len(), 50);
    assert_eq!(user_codes.len(), 50);
    drop(service);
    assert_eq!(harness.finish().await, 0, "leaked connections");
}
