//! AZ-3701 AC-5 (IT-06-8 audit and log, NFT-SEC-16): each trip of AC-1 and AC-2 writes exactly one `abuse_trip`
//! event and one WARN line, however many calls are refused after it; the per-link trip names the link, the
//! per-address trip a hash of the address; no captured line and no row holds a token or a password.

use contracts::{PublicShareError, PublicShareService};
use tracing::Level;

use crate::logs::LogCapture;
use crate::rig::{IP1, IP2, Rig, unknown};
use crate::visitor_support::PASSWORD;

const WRONG: &str = "a wrong guess";
const TRIPPED: &str = "share abuse limit tripped";

#[tokio::test]
async fn each_trip_writes_one_audit_event_and_one_warn_line_without_the_token() {
    // Arrange
    let mut rig = Rig::new().await;
    let link = rig.link(Some(PASSWORD)).await;
    let token_limit = usize::try_from(rig.harness.config.invalid_token_limit).expect("fits");
    let logs = LogCapture::install();

    // Act
    for n in 0..token_limit + 20 {
        let answer = rig.shares.open(&unknown(n), None, IP1).await;
        assert!(answer.is_ok() || matches!(answer, Err(PublicShareError::RateLimited { .. })));
    }
    for _ in 0..rig.harness.config.wrong_password_limit + 5 {
        let answer = rig.shares.verify_password(&link.token, WRONG, IP2).await;
        assert!(matches!(
            answer,
            Err(PublicShareError::WrongPassword | PublicShareError::RateLimited { .. })
        ));
    }
    let trips = rig.trips().await;
    let warnings = logs.at(Level::WARN, TRIPPED);
    let lines = logs.lines();

    // Assert
    assert_eq!(trips.len(), 2, "{trips:?}");
    let (ip_trip, link_trip) = (&trips[0], &trips[1]);
    assert_eq!((ip_trip.0.as_deref(), ip_trip.1), (None, None));
    assert_eq!(ip_trip.2.as_deref(), Some("203.0.113.1"));
    assert_eq!(ip_trip.3.as_deref(), Some(r#"{"limit": "share_token_ip"}"#));
    assert_eq!(
        (link_trip.0.as_deref(), link_trip.1),
        (Some("share_link"), Some(link.id))
    );
    assert_eq!(link_trip.2.as_deref(), Some("203.0.113.2"));
    assert_eq!(
        link_trip.3.as_deref(),
        Some(r#"{"limit": "share_password"}"#)
    );
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings[0].text.contains("share_token_ip"));
    assert!(warnings[0].text.contains("ip_hash="));
    assert!(!warnings[0].text.contains("203.0.113.1"), "{warnings:?}");
    assert!(warnings[1].text.contains("share_password"));
    assert!(warnings[1].text.contains(&link.id.to_string()));
    let secrets = [
        link.token.expose().to_owned(),
        unknown(0).expose().to_owned(),
        WRONG.to_owned(),
        PASSWORD.to_owned(),
    ];
    for line in &lines {
        for secret in &secrets {
            assert!(!line.text.contains(secret.as_str()), "{line:?}");
        }
    }
    rig.finish().await;
}
