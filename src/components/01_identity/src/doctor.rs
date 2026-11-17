//! The two `DoctorCheck`s of 01: the sign-in providers (the redirect URI to register and whether the provider
//! answers) and the setup state. Both only read, and neither prints a client secret or a setup token.

use std::sync::Arc;
use std::time::Duration;

use contracts::{DoctorCheck, DoctorContext, DoctorTag, Finding, FindingLevel, async_trait};
use sc_clock::Clock;
use sc_db::Db;
use sc_observability::log_error;

use crate::auth_repo;
use crate::config::IdentityConfig;
use crate::error::IdentityError;
use crate::provider::{self, ProviderClient, ProviderError};
use crate::sign_in::PublicUrl;

const SETUP_REMEDY: &str =
    "run `sunnycloud admin setup-token` and sign in with the token it prints";

fn finding(level: FindingLevel, message: String, remedy: String) -> Finding {
    Finding {
        level,
        message,
        remedy,
    }
}

/// Per configured provider: the redirect URI to register (the one `begin` sends) and whether the provider's
/// discovery or token endpoint answers within `provider_timeout`. Holds no database, so it runs in `--pre-flight`.
pub struct IdentityProvidersCheck {
    public_url: PublicUrl,
    providers: Vec<provider::Keyed>,
    timeout: Duration,
}

impl IdentityProvidersCheck {
    /// Builds the provider clients as sign-in does. Opens no connection.
    pub fn new(clock: Arc<dyn Clock>, config: &IdentityConfig) -> Result<Self, IdentityError> {
        Ok(Self::from_parts(
            PublicUrl::of(config)?,
            provider::build_all(config, &clock)?,
            config.provider_timeout,
        ))
    }

    pub(crate) fn from_parts(
        public_url: PublicUrl,
        providers: Vec<provider::Keyed>,
        timeout: Duration,
    ) -> Self {
        IdentityProvidersCheck {
            public_url,
            providers,
            timeout,
        }
    }

    async fn check(&self, key: &str, client: &dyn ProviderClient) -> Finding {
        let uri = self.public_url.callback_url(key);
        let outcome = match tokio::time::timeout(self.timeout, client.probe()).await {
            Ok(Ok(())) => {
                return finding(
                    FindingLevel::Ok,
                    format!("provider {key}: register {uri}; it answers"),
                    String::new(),
                );
            }
            Ok(Err(ProviderError::Unavailable(error))) => {
                log_error!("provider probe failed", err = &*error);
                "it did not answer".to_owned()
            }
            Ok(Err(ProviderError::Rejected(error))) => {
                log_error!("provider probe rejected", err = &*error);
                "its answer was not acceptable".to_owned()
            }
            Err(_) => format!("it did not answer within {} s", self.timeout.as_secs()),
        };
        finding(
            FindingLevel::Warn,
            format!("provider {key}: register {uri}; {outcome}"),
            format!(
                "check the network path to provider {key} and its issuer setting; sign-in through it fails until it answers"
            ),
        )
    }
}

#[async_trait]
impl DoctorCheck for IdentityProvidersCheck {
    fn name(&self) -> &'static str {
        "identity_providers"
    }

    fn tags(&self) -> &[DoctorTag] {
        &[DoctorTag::PreFlight]
    }

    async fn run(&self, _ctx: &DoctorContext) -> Vec<Finding> {
        if self.providers.is_empty() {
            return vec![finding(
                FindingLevel::Fail,
                "no sign-in provider is configured".to_owned(),
                "configure a GitHub or OIDC provider".to_owned(),
            )];
        }
        let mut findings = Vec::with_capacity(self.providers.len());
        for (key, client) in &self.providers {
            findings.push(self.check(key, client.as_ref()).await);
        }
        findings
    }
}

/// Whether the first admin exists. Pending setup is a normal state of a fresh install, so it is `Ok` with the
/// age of the open setup token or the command that makes one.
pub struct IdentitySetupCheck {
    db: Db,
    clock: Arc<dyn Clock>,
}

impl IdentitySetupCheck {
    pub fn new(db: Db, clock: Arc<dyn Clock>) -> Self {
        IdentitySetupCheck { db, clock }
    }
}

/// Whole days, else whole hours, else whole minutes.
fn age_text(age: Duration) -> String {
    let (days, hours, minutes) = (
        age.as_secs() / 86_400,
        age.as_secs() / 3_600,
        age.as_secs() / 60,
    );
    match (days, hours) {
        (1.., _) => format!("{days} day(s)"),
        (0, 1..) => format!("{hours} hour(s)"),
        _ => format!("{minutes} minute(s)"),
    }
}

#[async_trait]
impl DoctorCheck for IdentitySetupCheck {
    fn name(&self) -> &'static str {
        "identity_setup"
    }

    fn tags(&self) -> &[DoctorTag] {
        &[]
    }

    async fn run(&self, _ctx: &DoctorContext) -> Vec<Finding> {
        let now = self.clock.now_utc();
        let (accounts, token) = match auth_repo::setup_state(&self.db, now).await {
            Ok(state) => state,
            Err(error) => {
                log_error!("setup state read failed", err = &error);
                return vec![finding(
                    FindingLevel::Fail,
                    "the setup state could not be read".to_owned(),
                    "run doctor again once the database answers".to_owned(),
                )];
            }
        };
        if accounts {
            return vec![finding(
                FindingLevel::Ok,
                "setup is done: an account exists".to_owned(),
                String::new(),
            )];
        }
        let message = match token {
            Some(created) => {
                let age = u64::try_from(now.as_unix_micros().saturating_sub(created))
                    .map_or(Duration::ZERO, Duration::from_micros);
                format!(
                    "setup pending: no admin yet; a setup token was opened {} ago",
                    age_text(age)
                )
            }
            None => "setup pending: no admin yet and no open setup token".to_owned(),
        };
        vec![finding(FindingLevel::Ok, message, SETUP_REMEDY.to_owned())]
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    const KEY: &str = "gh";
    const TIMEOUT: Duration = Duration::from_millis(50);

    enum Behaviour {
        Answers,
        Unavailable,
        Rejected,
        Stalls,
    }

    struct Probed(Behaviour);

    #[async_trait]
    impl ProviderClient for Probed {
        async fn authorization_url(
            &self,
            _: &crate::provider::AuthorizationRequest<'_>,
        ) -> Result<String, ProviderError> {
            unreachable!("doctor never begins a sign-in")
        }

        async fn identity(
            &self,
            _: &crate::provider::CodeExchange<'_>,
        ) -> Result<crate::provider::ProviderIdentity, ProviderError> {
            unreachable!("doctor never redeems a code")
        }

        async fn probe(&self) -> Result<(), ProviderError> {
            let cause = || -> Box<dyn Error + Send + Sync> { "secret-looking detail".into() };
            match self.0 {
                Behaviour::Answers => Ok(()),
                Behaviour::Unavailable => Err(ProviderError::Unavailable(cause())),
                Behaviour::Rejected => Err(ProviderError::Rejected(cause())),
                Behaviour::Stalls => {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    Ok(())
                }
            }
        }
    }

    fn check_of(behaviour: Behaviour) -> IdentityProvidersCheck {
        let public_url = PublicUrl::parse("https://sunnycloud.test").expect("public url");
        IdentityProvidersCheck::from_parts(
            public_url,
            vec![(KEY.to_owned(), Arc::new(Probed(behaviour)))],
            TIMEOUT,
        )
    }

    async fn run(check: &IdentityProvidersCheck) -> Vec<Finding> {
        check
            .run(&DoctorContext {
                pre_flight: true,
                backup_interval: None,
                attest_backup: false,
            })
            .await
    }

    #[tokio::test]
    async fn every_probe_result_prints_the_redirect_uri_and_only_an_answer_is_ok() {
        // Arrange
        let cases = [
            (Behaviour::Answers, FindingLevel::Ok),
            (Behaviour::Unavailable, FindingLevel::Warn),
            (Behaviour::Rejected, FindingLevel::Warn),
            (Behaviour::Stalls, FindingLevel::Warn),
        ];

        for (behaviour, level) in cases {
            // Act
            let findings = run(&check_of(behaviour)).await;

            // Assert
            assert_eq!(findings.len(), 1);
            assert_eq!(findings[0].level, level);
            assert!(
                findings[0]
                    .message
                    .contains("https://sunnycloud.test/api/v1/auth/providers/gh/callback"),
                "{}",
                findings[0].message
            );
        }
    }

    #[tokio::test]
    async fn a_provider_that_is_not_ok_names_its_key_in_the_remedy_and_leaks_no_error_text() {
        // Arrange
        let check = check_of(Behaviour::Unavailable);

        // Act
        let findings = run(&check).await;

        // Assert
        assert!(findings[0].remedy.contains(KEY), "{}", findings[0].remedy);
        assert!(!findings[0].message.contains("secret-looking"));
        assert!(!findings[0].remedy.contains("secret-looking"));
    }

    #[tokio::test]
    async fn no_provider_at_all_is_a_fail() {
        // Arrange
        let public_url = PublicUrl::parse("https://sunnycloud.test").expect("public url");
        let check = IdentityProvidersCheck::from_parts(public_url, Vec::new(), TIMEOUT);

        // Act
        let findings = run(&check).await;

        // Assert
        assert_eq!(findings[0].level, FindingLevel::Fail);
    }

    #[test]
    fn the_token_age_is_given_in_whole_days_hours_or_minutes() {
        // Arrange
        let ages = [
            Duration::from_secs(2 * 86_400 + 5),
            Duration::from_secs(3 * 3_600 + 59),
            Duration::from_secs(90),
        ];

        // Act
        let texts = ages.map(age_text);

        // Assert
        assert_eq!(texts, ["2 day(s)", "3 hour(s)", "1 minute(s)"]);
    }
}
