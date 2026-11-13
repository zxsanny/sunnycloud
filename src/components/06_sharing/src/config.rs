use std::path::PathBuf;
use std::time::Duration;

use argon2::Params;
use sc_config::{ConfigError, ConfigSources, KeyProblem, Reason, Secret, Settings, TABLE};

pub const GRANT_KEY_KEY: &str = "share_grant_key";
pub const GRANT_KEY_MIN_BYTES: usize = 32;
pub const ARGON2_PARAMS_KEY: &str = "share_argon2_params";

/// The sharing section of the key table, typed. Defaults and the production duration ranges come from
/// `sc_config`. The grant key stays a `Secret`: it prints as `***` and never enters a problem.
#[derive(Debug, Clone)]
pub struct ShareConfig {
    pub default_ttl: Duration,
    pub max_ttl: Duration,
    pub lookup_cache: Duration,
    pub grant_ttl: Duration,
    pub wrong_password_limit: u64,
    pub wrong_password_window: Duration,
    pub invalid_token_limit: u64,
    /// No default in the key table: absent means the password path sets its own bound.
    pub password_max_concurrency: Option<u64>,
    /// No default in the key table (`m,t,p`); unset means argon2's `Params::DEFAULT` (19456 KiB, 2 passes,
    /// 1 lane). Read through [`ShareConfig::password_params`].
    pub argon2_params: Option<String>,
    pub grant_key: Secret<String>,
}

impl ShareConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        ShareConfig {
            default_ttl: settings.share_default_ttl.get(),
            max_ttl: settings.share_max_ttl.get(),
            lookup_cache: settings.share_lookup_cache.get(),
            grant_ttl: settings.share_grant_ttl.get(),
            wrong_password_limit: settings.share_wrong_password_limit,
            wrong_password_window: settings.share_wrong_password_window.get(),
            invalid_token_limit: settings.share_invalid_token_limit,
            password_max_concurrency: settings.share_password_max_concurrency,
            argon2_params: settings.share_argon2_params.clone(),
            grant_key: settings.share_grant_key.clone(),
        }
    }

    /// Start validation, the single entry point for the grant key rule: every problem, each naming its key.
    /// `sc_config` binds the duration ranges only in production, so zero values that would make every link or
    /// grant expire at once, or refuse every attempt, are refused in every environment. A default lifetime above
    /// the maximum would make every default create fail with `ExpiryOutOfRange`.
    pub fn validate(&self) -> Vec<KeyProblem> {
        let mut problems = Vec::new();
        for (key, value) in [
            ("share_default_ttl", self.default_ttl),
            ("share_max_ttl", self.max_ttl),
            ("share_grant_ttl", self.grant_ttl),
            ("share_wrong_password_window", self.wrong_password_window),
        ] {
            if value.is_zero() {
                problems.push(out_of_range(key, "1ms", "no ceiling"));
            }
        }
        if self.default_ttl > self.max_ttl {
            problems.push(out_of_range("share_default_ttl", "1ms", "share_max_ttl"));
        }
        for (key, value) in [
            ("share_wrong_password_limit", self.wrong_password_limit),
            ("share_invalid_token_limit", self.invalid_token_limit),
        ] {
            if value == 0 {
                problems.push(out_of_range(key, "1", "no ceiling"));
            }
        }
        if self.password_max_concurrency == Some(0) {
            problems.push(out_of_range(
                "share_password_max_concurrency",
                "1",
                "no ceiling",
            ));
        }
        problems.extend(self.password_params().err());
        problems.extend(self.grant_key_problem());
        problems
    }

    /// The Argon2id cost of link passwords: `share_argon2_params` as `m,t,p` (memory in KiB, passes, lanes), or
    /// argon2's `Params::DEFAULT` when unset. A value that is not three integers, or that argon2 refuses (memory
    /// below 8 KiB per lane, zero passes or lanes), is a problem naming the key; the value is not repeated.
    pub fn password_params(&self) -> Result<Params, KeyProblem> {
        let Some(text) = self.argon2_params.as_deref() else {
            return Ok(Params::DEFAULT);
        };
        let parts: Vec<Option<u32>> = text
            .split(',')
            .map(|part| part.trim().parse().ok())
            .collect();
        let wrong_type = || KeyProblem::new(ARGON2_PARAMS_KEY, Reason::WrongType);
        match parts.as_slice() {
            [Some(m), Some(t), Some(p)] => Params::new(*m, *t, *p, None).map_err(|_| wrong_type()),
            _ => Err(wrong_type()),
        }
    }

    /// A key that the loader refused (no value, empty file, file that cannot be read) is a problem naming
    /// `share_grant_key`, so a refused start reads the same whichever layer saw the key first. `None` when the
    /// error is about something else.
    pub fn load_refusal(error: &ConfigError, sources: &ConfigSources) -> Option<KeyProblem> {
        let refused = match error {
            ConfigError::MissingSecret(key) => key == GRANT_KEY_KEY,
            ConfigError::Unreadable(path, _) => *path == grant_key_file(sources),
            ConfigError::Invalid(problems) => {
                return problems.iter().find(|p| p.key == GRANT_KEY_KEY).cloned();
            }
        };
        refused.then(|| KeyProblem::new(GRANT_KEY_KEY, Reason::MissingRequired))
    }

    fn grant_key_problem(&self) -> Option<KeyProblem> {
        match self.grant_key.expose().len() {
            0 => Some(KeyProblem::new(GRANT_KEY_KEY, Reason::MissingRequired)),
            GRANT_KEY_MIN_BYTES.. => None,
            _ => Some(out_of_range(GRANT_KEY_KEY, "32B", "no ceiling")),
        }
    }
}

/// The file the loader reads the grant key from: `SUNNYCLOUD_SHARE_GRANT_KEY_FILE`, else the secrets folder.
fn grant_key_file(sources: &ConfigSources) -> PathBuf {
    let spec = TABLE.iter().find(|spec| spec.key == GRANT_KEY_KEY);
    match spec.and_then(|spec| sources.env.get(&spec.file_env_name())) {
        Some(path) => PathBuf::from(path),
        None => sources.secrets_dir.join(GRANT_KEY_KEY),
    }
}

fn out_of_range(key: &str, min: &str, max: &str) -> KeyProblem {
    KeyProblem::new(
        key,
        Reason::OutOfRange {
            min: min.to_owned(),
            max: max.to_owned(),
        },
    )
}
