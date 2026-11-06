use std::time::Duration;

use sc_config::{KeyProblem, Reason, Secret, Settings};

const PROVIDER_KINDS: [&str; 2] = ["github", "oidc"];
const DEVICE_CODE_TTL_CEILING: Duration = Duration::from_secs(600);
/// Above this the session cache outlives the 5 s revocation bound (AC-21).
const SESSION_CACHE_TTL_CEILING: Duration = Duration::from_secs(3);

/// One configured sign-in provider.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// The key the provider is addressed by in callback paths.
    pub key: String,
    /// `github` or `oidc`; any other text is refused by [`IdentityConfig::validate`].
    pub kind: String,
    /// OIDC issuer; for `github` an override of the provider's base URL (used to point at mock-idp).
    pub issuer: Option<String>,
    pub client_id: String,
    pub client_secret: Secret<String>,
}

/// The identity section of the key table, typed. Defaults and per-key ranges come from `sc_config`.
#[derive(Debug, Clone)]
pub struct IdentityConfig {
    pub public_url: String,
    pub providers: Vec<ProviderConfig>,
    pub email_allowlist: Vec<String>,
    pub access_token_ttl: Duration,
    pub refresh_token_ttl: Duration,
    pub refresh_reuse_grace: Duration,
    pub session_cache_ttl: Duration,
    pub session_last_seen_interval: Duration,
    pub device_code_ttl: Duration,
    pub device_poll_interval: Duration,
    pub oauth_state_ttl: Duration,
    pub login_code_ttl: Duration,
    pub invite_ttl: Duration,
    pub setup_token_ttl: Duration,
    pub provider_timeout: Duration,
    pub device_grant_ip_limit: Option<u64>,
    pub invalid_token_ip_limit: Option<u64>,
    pub geoip_db_path: Option<String>,
    pub allow_plain_http_dev: bool,
}

impl IdentityConfig {
    /// A provider is configured when all of its keys are set; a half-configured one is reported by
    /// `ApiSettings::validate` through the keys it lacks.
    pub fn from_settings(settings: &Settings) -> Self {
        let mut providers = Vec::new();
        if let (Some(client_id), Some(secret)) = (
            &settings.github_client_id,
            &settings.provider_github_client_secret,
        ) {
            providers.push(ProviderConfig {
                key: "github".to_owned(),
                kind: "github".to_owned(),
                issuer: None,
                client_id: client_id.clone(),
                client_secret: secret.clone(),
            });
        }
        if let (Some(issuer), Some(client_id), Some(secret)) = (
            &settings.oidc_issuer,
            &settings.oidc_client_id,
            &settings.provider_oidc_client_secret,
        ) {
            providers.push(ProviderConfig {
                key: "oidc".to_owned(),
                kind: "oidc".to_owned(),
                issuer: Some(issuer.clone()),
                client_id: client_id.clone(),
                client_secret: secret.clone(),
            });
        }
        IdentityConfig {
            public_url: settings.public_url.clone(),
            providers,
            email_allowlist: settings.allowlist_emails.clone().unwrap_or_default(),
            access_token_ttl: settings.access_token_ttl.get(),
            refresh_token_ttl: settings.refresh_token_ttl.get(),
            refresh_reuse_grace: settings.refresh_reuse_grace.get(),
            session_cache_ttl: settings.session_cache_ttl.get(),
            session_last_seen_interval: settings.session_last_seen_interval.get(),
            device_code_ttl: settings.device_code_ttl.get(),
            device_poll_interval: settings.device_poll_interval.get(),
            oauth_state_ttl: settings.oauth_state_ttl.get(),
            login_code_ttl: settings.login_code_ttl.get(),
            invite_ttl: settings.invite_ttl.get(),
            setup_token_ttl: settings.setup_token_ttl.get(),
            provider_timeout: settings.provider_timeout.get(),
            device_grant_ip_limit: settings.device_grant_ip_limit,
            invalid_token_ip_limit: settings.invalid_token_ip_limit,
            geoip_db_path: settings.geoip_db_path.clone(),
            allow_plain_http_dev: settings.allow_plain_http_dev,
        }
    }

    /// Start validation: every problem, each naming its key. The ceilings bind in every environment.
    pub fn validate(&self) -> Vec<KeyProblem> {
        let mut problems = Vec::new();
        self.check_public_url(&mut problems);
        self.check_providers(&mut problems);
        check_ceiling(
            "device_code_ttl",
            self.device_code_ttl,
            DEVICE_CODE_TTL_CEILING,
            "10m",
            &mut problems,
        );
        check_ceiling(
            "session_cache_ttl",
            self.session_cache_ttl,
            SESSION_CACHE_TTL_CEILING,
            "3s",
            &mut problems,
        );
        problems
    }

    /// `https`, or `http` only with the development flag. A relative or other-scheme URL is malformed.
    fn check_public_url(&self, problems: &mut Vec<KeyProblem>) {
        let lower = self.public_url.to_ascii_lowercase();
        let reason = if lower.starts_with("https://") {
            return;
        } else if lower.starts_with("http://") {
            if self.allow_plain_http_dev {
                return;
            }
            Reason::ForbiddenInRelease
        } else {
            Reason::WrongType
        };
        problems.push(KeyProblem::new("public_url", reason));
    }

    fn check_providers(&self, problems: &mut Vec<KeyProblem>) {
        if self.providers.is_empty() {
            problems.push(KeyProblem::new("providers", Reason::MissingRequired));
        }
        for (index, provider) in self.providers.iter().enumerate() {
            if !PROVIDER_KINDS.contains(&provider.kind.as_str()) {
                problems.push(KeyProblem::new(
                    format!("providers[{index}].kind"),
                    Reason::WrongType,
                ));
            }
        }
    }
}

fn check_ceiling(
    key: &str,
    value: Duration,
    ceiling: Duration,
    ceiling_text: &str,
    problems: &mut Vec<KeyProblem>,
) {
    if value > ceiling {
        problems.push(KeyProblem::new(
            key,
            Reason::OutOfRange {
                min: "0s".to_owned(),
                max: ceiling_text.to_owned(),
            },
        ));
    }
}
