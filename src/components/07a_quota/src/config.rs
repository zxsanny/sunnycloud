use sc_config::{KeyProblem, Reason, Settings};

/// The largest limit a quota may carry (2^53 bytes), for an account limit and for the installation default.
pub const MAX_LIMIT_BYTES: i64 = 1 << 53;

/// The quota section of the key table, typed. The default (100 GiB) comes from `sc_config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaConfig {
    /// Limit of an account whose `limit_bytes` is NULL. Saturates at `i64::MAX`, which `validate` refuses.
    pub default_quota_bytes: i64,
}

impl QuotaConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        QuotaConfig {
            default_quota_bytes: i64::try_from(settings.default_quota.bytes()).unwrap_or(i64::MAX),
        }
    }

    /// Start validation: every problem, each naming its key.
    pub fn validate(&self) -> Vec<KeyProblem> {
        let mut problems = Vec::new();
        if !(0..=MAX_LIMIT_BYTES).contains(&self.default_quota_bytes) {
            problems.push(KeyProblem::new(
                "default_quota",
                Reason::OutOfRange {
                    min: "0B".to_owned(),
                    max: "8192TiB".to_owned(),
                },
            ));
        }
        problems
    }
}
