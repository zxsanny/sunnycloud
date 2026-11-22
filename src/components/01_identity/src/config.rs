use std::time::Duration;

use sc_config::{KeyProblem, Reason, Secret, Settings};

const PROVIDER_KINDS: [&str; 3] = ["github", "oidc", "apple"];
/// Apple Team IDs are 10 characters, letters and digits.
const APPLE_TEAM_ID_LEN: usize = 10;
const DEVICE_CODE_TTL_CEILING: Duration = Duration::from_secs(600);
/// Above this the session cache outlives the 5 s revocation bound (AC-21).
const SESSION_CACHE_TTL_CEILING: Duration = Duration::from_secs(3);

/// One configured sign-in provider.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// The key the provider is addressed by in callback paths.
    pub key: String,
    /// `github`, `oidc` or `apple`; any other text is refused by [`IdentityConfig::validate`].
    pub kind: String,
    /// OIDC issuer; for `github` an override of the provider's base URL (used to point at mock-idp); for
    /// `apple` an override of `https://appleid.apple.com`.
    pub issuer: Option<String>,
    /// For `apple` the Services ID.
    pub client_id: String,
    /// For `apple` the contents of the `.p8` private key file; the client secret sent to Apple is derived from
    /// it for every exchange.
    pub client_secret: Secret<String>,
    /// `apple` only: the 10-character Team ID, the `iss` of the client-secret JWT.
    pub team_id: Option<String>,
    /// `apple` only: the key id of the `.p8` key, the `kid` header of the client-secret JWT.
    pub key_id: Option<String>,
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
                team_id: None,
                key_id: None,
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
                team_id: None,
                key_id: None,
            });
        }
        if let (Some(client_id), Some(team_id), Some(key_id), Some(p8)) = (
            &settings.apple_client_id,
            &settings.apple_team_id,
            &settings.apple_key_id,
            &settings.apple_p8,
        ) {
            providers.push(ProviderConfig {
                key: "apple".to_owned(),
                kind: "apple".to_owned(),
                issuer: None,
                client_id: client_id.clone(),
                client_secret: p8.clone(),
                team_id: Some(team_id.clone()),
                key_id: Some(key_id.clone()),
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
            } else if provider.kind == "apple" {
                check_apple(index, provider, problems);
            }
        }
    }
}

/// An `apple` provider needs its `.p8` key, its 10-character Team ID and its key id; each problem names the
/// provider's entry (whether the key file parses is checked when the client is built).
fn check_apple(index: usize, provider: &ProviderConfig, problems: &mut Vec<KeyProblem>) {
    let mut missing = |field: &str, reason: Reason| {
        problems.push(KeyProblem::new(
            format!("providers[{index}].{field}"),
            reason,
        ));
    };
    if provider.client_secret.expose().trim().is_empty() {
        missing("client_secret", Reason::MissingRequired);
    }
    match provider.team_id.as_deref() {
        None | Some("") => missing("team_id", Reason::MissingRequired),
        Some(team_id) if !is_team_id(team_id) => missing("team_id", Reason::WrongType),
        Some(_) => {}
    }
    if provider.key_id.as_deref().is_none_or(str::is_empty) {
        missing("key_id", Reason::MissingRequired);
    }
}

fn is_team_id(text: &str) -> bool {
    text.len() == APPLE_TEAM_ID_LEN && text.chars().all(|c| c.is_ascii_alphanumeric())
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
