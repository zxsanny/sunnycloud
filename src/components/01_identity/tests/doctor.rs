//! AZ-4022 against the real provider clients and PostgreSQL: the redirect URI each finding prints equals the one
//! `begin` sends, a provider that stalls or sits at a refused address is a warning inside the timeout, and the
//! setup state follows the rows without changing any.

mod support;

use std::time::{Duration, Instant};

use contracts::{DoctorCheck, DoctorContext, Finding, FindingLevel, OperatorIdentityOps};
use reqwest::Url;
use sunnycloud_identity::{IdentityProvidersCheck, IdentitySetupCheck};
use support::START;
use support::fixtures;
use support::invites::{invites, token_rows};
use support::sign_in::{GITHUB, OIDC, World, world_with};
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_millis(300);
const PLAIN: DoctorContext = DoctorContext {
    pre_flight: false,
    backup_interval: None,
    attest_backup: false,
};

fn providers_check(world: &World) -> IdentityProvidersCheck {
    IdentityProvidersCheck::new(world.harness.clock.clone(), &world.harness.config)
        .expect("providers check")
}

fn redirect_uri_of(authorization_url: &str) -> String {
    Url::parse(authorization_url)
        .expect("authorization url")
        .query_pairs()
        .find(|(name, _)| name == "redirect_uri")
        .expect("redirect_uri in the url")
        .1
        .into_owned()
}

fn about<'a>(findings: &'a [Finding], key: &str) -> &'a Finding {
    findings
        .iter()
        .find(|finding| finding.message.starts_with(&format!("provider {key}:")))
        .expect("a finding for the provider")
}

#[tokio::test]
async fn each_provider_finding_prints_the_redirect_uri_begin_sends_and_both_clients_answer() {
    // Arrange
    let world = world_with(|config| config.provider_timeout = TIMEOUT).await;
    let check = providers_check(&world);
    let (github_url, _) = world.begin(GITHUB).await;
    let (oidc_url, _) = world.begin(OIDC).await;

    // Act
    let findings = check.run(&PLAIN).await;

    // Assert
    assert_eq!(findings.len(), 2);
    for (key, url) in [(GITHUB, github_url), (OIDC, oidc_url)] {
        let finding = about(&findings, key);
        assert_eq!(finding.level, FindingLevel::Ok, "{}", finding.message);
        assert!(
            finding.message.contains(&redirect_uri_of(&url)),
            "{}",
            finding.message
        );
    }
    world.finish().await;
}

#[tokio::test]
async fn a_stalled_provider_and_a_refused_address_are_warnings_inside_the_timeout() {
    // Arrange
    let world = world_with(|config| {
        config.provider_timeout = TIMEOUT;
        config.providers[1].issuer = Some("http://169.254.169.254".to_owned());
    })
    .await;
    let check = providers_check(&world);
    world
        .idp
        .behave(|behaviour| behaviour.stall_token_endpoint = true);

    // Act
    let started = Instant::now();
    let findings = check.run(&PLAIN).await;
    let elapsed = started.elapsed();

    // Assert
    assert!(
        elapsed < 2 * TIMEOUT + Duration::from_secs(1),
        "{elapsed:?}"
    );
    for key in [GITHUB, OIDC] {
        let finding = about(&findings, key);
        assert_eq!(finding.level, FindingLevel::Warn, "{}", finding.message);
        assert!(
            finding
                .message
                .contains(&format!("/providers/{key}/callback"))
        );
        assert!(finding.remedy.contains(key), "{}", finding.remedy);
    }
    world.finish().await;
}

#[tokio::test]
async fn no_finding_holds_a_client_secret() {
    // Arrange
    let world = world_with(|config| config.provider_timeout = TIMEOUT).await;
    let check = providers_check(&world);
    world
        .idp
        .behave(|behaviour| behaviour.stall_token_endpoint = true);

    // Act
    let findings = check.run(&PLAIN).await;

    // Assert
    for finding in findings {
        assert!(!format!("{finding:?}").contains("provider-secret"));
    }
    world.finish().await;
}

#[tokio::test]
async fn the_setup_state_follows_the_token_and_the_accounts_and_changes_no_row() {
    // Arrange
    let mut world = world_with(|_| {}).await;
    let check = IdentitySetupCheck::new(world.harness.api.clone(), world.harness.clock.clone());
    let without_token = check.run(&PLAIN).await;
    let operator = invites(&world.harness);
    let token = operator
        .ensure_setup_token()
        .await
        .expect("ensure")
        .expect("a token while no account exists");
    world.harness.clock.tick(Duration::from_secs(2 * 86_400));
    let rows_before = token_rows(&mut world.conn).await;

    // Act
    let with_token = check.run(&PLAIN).await;
    let rows_after = token_rows(&mut world.conn).await;
    fixtures::account_in(
        &mut world.conn,
        Uuid::new_v4(),
        "admin",
        "active",
        START.as_unix_micros(),
    )
    .await;
    let with_admin = check.run(&PLAIN).await;

    // Assert
    assert_eq!(without_token[0].level, FindingLevel::Ok);
    assert!(without_token[0].message.contains("no open setup token"));
    assert!(without_token[0].remedy.contains("admin setup-token"));
    assert_eq!(with_token[0].level, FindingLevel::Ok);
    assert!(
        with_token[0].message.contains("2 day(s)"),
        "{}",
        with_token[0].message
    );
    assert!(!format!("{with_token:?}").contains(token.expose()));
    assert_eq!(rows_before, rows_after);
    assert_eq!(with_admin[0].level, FindingLevel::Ok);
    assert!(with_admin[0].message.contains("setup is done"));
    drop((check, operator));
    world.finish().await;
}

#[tokio::test]
async fn an_expired_setup_token_is_no_open_token() {
    // Arrange
    let world = world_with(|_| {}).await;
    let check = IdentitySetupCheck::new(world.harness.api.clone(), world.harness.clock.clone());
    invites(&world.harness)
        .ensure_setup_token()
        .await
        .expect("ensure");
    world
        .harness
        .clock
        .tick(world.harness.config.setup_token_ttl + Duration::from_secs(1));

    // Act
    let findings = check.run(&PLAIN).await;

    // Assert
    assert!(
        findings[0].message.contains("no open setup token"),
        "{}",
        findings[0].message
    );
    drop(check);
    world.finish().await;
}
