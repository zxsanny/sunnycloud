//! Redirects (owner decision 2026-11-12): followed only to listed hosts, over HTTPS, at most `MAX_HOPS`.
//! In the test build the main host and `cdn` stand in for the listed GitHub hosts.

use reqwest::Url;
use tracing::Level;

use super::host::{RELEASES, ReleaseHost, Reply};
use super::{Harness, Release, TestKey, asset_name, harness, payload};
use crate::updater::manifest::{MANIFEST_NAME, SIGNATURE_NAME};
use crate::updater::{MAX_HOPS, RedirectRule};

/// Serves `body` for `path` on the main host through `hops` redirects that alternate between `cdn` and the
/// main host.
fn redirect_chain(h: &Harness, path: &str, hops: usize, body: Vec<u8>) {
    let mut at = (&h.host, path.to_owned());
    for hop in 1..=hops {
        let next = if hop % 2 == 1 { &h.cdn } else { &h.host };
        let next_path = format!("/hop{hop}{path}");
        at.0.route(&at.1, Reply::Redirect(next.url(&next_path)));
        at = (next, next_path);
    }
    at.0.route(&at.1, Reply::Body(body));
}

fn asset_path(version: &str) -> String {
    format!("{RELEASES}/download/v{version}/{}", asset_name(version))
}

fn source_warns(h: &Harness) -> usize {
    h.captured
        .events()
        .iter()
        .filter(|(level, line)| *level == Level::WARN && line.contains("check=\"source\""))
        .count()
}

#[tokio::test]
async fn check_once_redirects_to_listed_hosts_are_followed_up_to_the_limit() {
    // Arrange
    let h = harness().await;
    let good = Release::signed("1.0.1", 7, TestKey::A);
    h.publish(&good);
    let manifest_path = format!("{RELEASES}/latest/download/{MANIFEST_NAME}");
    redirect_chain(&h, &manifest_path, 1, good.manifest.clone());
    redirect_chain(&h, &asset_path("1.0.1"), MAX_HOPS, payload());

    // Act
    let staged = h
        .updater
        .check_once()
        .await
        .expect("listed redirects followed");

    // Assert
    assert_eq!((staged.version.as_str(), staged.serial), ("1.0.1", 7));
    assert_eq!(std::fs::read(&staged.path).expect("staged"), payload());
    assert_eq!(h.accepted_serial(), Some(7));
    assert!(!h.cdn.requests().is_empty());
}

#[tokio::test]
async fn check_once_redirect_to_an_unlisted_host_is_refused() {
    // Arrange
    let h = harness().await;
    let elsewhere = ReleaseHost::start().await;
    let good = Release::signed("1.0.1", 7, TestKey::A);
    elsewhere.set_latest(MANIFEST_NAME, Some(Reply::Body(good.manifest.clone())));
    elsewhere.set_latest(SIGNATURE_NAME, good.signature.clone().map(Reply::Body));
    h.host.set_latest(
        MANIFEST_NAME,
        Some(Reply::Redirect(elsewhere.latest_url(MANIFEST_NAME))),
    );

    // Act
    let error = h.updater.check_once().await.expect_err("redirect refused");

    // Assert
    assert_eq!(error.check(), "source", "{error}");
    assert!(
        error.to_string().contains("outside the release list"),
        "{error}"
    );
    assert!(elsewhere.requests().is_empty());
    assert!(h.staging_files().is_empty());
    assert_eq!(h.accepted_serial(), None);
    assert_eq!(source_warns(&h), 1);
}

#[tokio::test]
async fn check_once_more_hops_than_allowed_is_refused() {
    // Arrange
    let h = harness().await;
    h.publish(&Release::signed("1.0.1", 7, TestKey::A));
    redirect_chain(&h, &asset_path("1.0.1"), MAX_HOPS + 1, payload());

    // Act
    let error = h.updater.check_once().await.expect_err("too many hops");

    // Assert
    assert_eq!(error.check(), "source", "{error}");
    assert!(
        error
            .to_string()
            .contains(&format!("more than {MAX_HOPS} redirects")),
        "{error}"
    );
    let last_hop = format!("/hop{}{}", MAX_HOPS + 1, asset_path("1.0.1"));
    let fetched = [h.host.requests(), h.cdn.requests()].concat();
    assert!(!fetched.contains(&last_hop), "{fetched:?}");
    assert!(h.staging_files().is_empty());
    assert_eq!(h.accepted_serial(), None);
    assert_eq!(source_warns(&h), 1);
}

#[test]
fn release_rule_allows_only_listed_https_hosts_within_the_hop_limit() {
    // Arrange
    let rule = RedirectRule::release();
    let cases = [
        (
            "https://github.com/zxsanny/sunnycloud/releases/download/v1.0.1/a.deb",
            1,
            true,
        ),
        (
            "https://release-assets.githubusercontent.com/a?sig=x",
            2,
            true,
        ),
        ("https://objects.githubusercontent.com/a", MAX_HOPS, true),
        (
            "https://release-assets.githubusercontent.com/a",
            MAX_HOPS + 1,
            false,
        ),
        ("http://github.com/a", 1, false),
        ("http://release-assets.githubusercontent.com/a", 1, false),
        ("https://github.com.evil.example/a", 1, false),
        ("https://evil.example/github.com", 1, false),
        ("https://github.com:8443/a", 1, false),
        ("https://sunnycloud.example/api/v1/updates", 1, false),
    ];

    for (url, hop, expected) in cases {
        // Act
        let allowed = rule.check(&Url::parse(url).expect("url"), hop).is_ok();

        // Assert
        assert_eq!(allowed, expected, "{url} at hop {hop}");
    }
}
