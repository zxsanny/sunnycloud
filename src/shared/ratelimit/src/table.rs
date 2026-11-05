use std::net::IpAddr;
use std::time::Duration;

use uuid::Uuid;

/// Default for `ratelimit_max_entries`.
pub const DEFAULT_MAX_ENTRIES: usize = 100_000;

/// One abuse limit; each maps to one `limit` label value of `auth_rate_limited_total`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitId {
    SigninIp,
    DeviceGrantIp,
    DeviceApprovalSession,
    DeviceApprovalAccount,
    DeviceApprovalIp,
    InvalidTokenIp,
    SharePasswordLink,
    ShareTokenIp,
}

impl LimitId {
    pub const ALL: [LimitId; LIMIT_COUNT] = [
        LimitId::SigninIp,
        LimitId::DeviceGrantIp,
        LimitId::DeviceApprovalSession,
        LimitId::DeviceApprovalAccount,
        LimitId::DeviceApprovalIp,
        LimitId::InvalidTokenIp,
        LimitId::SharePasswordLink,
        LimitId::ShareTokenIp,
    ];

    /// The `limit` label value of `auth_rate_limited_total` (`deployment/observability.md`).
    pub const fn label(self) -> &'static str {
        match self {
            LimitId::SigninIp => "signin_ip",
            LimitId::DeviceGrantIp => "device_grant_ip",
            LimitId::DeviceApprovalSession => "device_approval_session",
            LimitId::DeviceApprovalAccount => "device_approval_account",
            LimitId::DeviceApprovalIp => "device_approval_ip",
            LimitId::InvalidTokenIp => "invalid_token_ip",
            LimitId::SharePasswordLink => "share_password",
            LimitId::ShareTokenIp => "share_token_ip",
        }
    }

    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

pub(crate) const LIMIT_COUNT: usize = 8;

/// Who is being limited. Never a name, email or token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Subject {
    Ip(IpAddr),
    Session(Uuid),
    Account(Uuid),
    Link(Uuid),
}

/// `capacity` failures per `window`; the bucket refills at `capacity / window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitSpec {
    pub capacity: u32,
    pub window: Duration,
}

impl LimitSpec {
    pub const fn new(capacity: u32, window: Duration) -> Self {
        Self { capacity, window }
    }
}

/// The construction error of [`crate::AbuseLimiter::new`]; named in the spec as `ConfigError`, kept
/// inside this crate (it is not `sc_config::ConfigError`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("limit {} has a zero capacity", .0.label())]
    ZeroCapacity(LimitId),
    #[error("limit {} has a zero window", .0.label())]
    ZeroWindow(LimitId),
    #[error("ratelimit_max_entries must be at least 1")]
    ZeroMaxEntries,
}

/// One [`LimitSpec`] per [`LimitId`] plus the bucket cap (`ratelimit_max_entries`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitTable {
    specs: [LimitSpec; LIMIT_COUNT],
    max_entries: usize,
}

impl LimitTable {
    /// AC-20 numbers for the limits that have them. `DeviceGrantIp` and `InvalidTokenIp` have no AC-20
    /// number and no config default (`device_grant_ip_limit`, `invalid_token_ip_limit` are unset), so the
    /// caller supplies both specs.
    pub fn new(device_grant_ip: LimitSpec, invalid_token_ip: LimitSpec) -> Self {
        const MINUTE: Duration = Duration::from_secs(60);
        const HOUR: Duration = Duration::from_secs(3600);
        Self {
            specs: [
                LimitSpec::new(10, MINUTE),
                device_grant_ip,
                LimitSpec::new(5, HOUR),
                LimitSpec::new(20, HOUR),
                LimitSpec::new(50, HOUR),
                invalid_token_ip,
                LimitSpec::new(10, Duration::from_secs(600)),
                LimitSpec::new(100, MINUTE),
            ],
            max_entries: DEFAULT_MAX_ENTRIES,
        }
    }

    /// Replaces one limit, e.g. the share limits from `share_wrong_password_limit` and friends.
    #[must_use]
    pub fn with_spec(mut self, limit: LimitId, spec: LimitSpec) -> Self {
        self.specs[limit.index()] = spec;
        self
    }

    /// Sets the bucket cap (`ratelimit_max_entries`).
    #[must_use]
    pub fn with_max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries;
        self
    }

    pub fn spec(&self, limit: LimitId) -> LimitSpec {
        self.specs[limit.index()]
    }

    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        for limit in LimitId::ALL {
            let spec = self.spec(limit);
            if spec.capacity == 0 {
                return Err(ConfigError::ZeroCapacity(limit));
            }
            if spec.window.is_zero() {
                return Err(ConfigError::ZeroWindow(limit));
            }
        }
        if self.max_entries == 0 {
            return Err(ConfigError::ZeroMaxEntries);
        }
        Ok(())
    }
}

/// Result of [`crate::AbuseLimiter::check`] and [`crate::AbuseLimiter::record_failure`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    /// This call exhausted the bucket: the caller audits once.
    Tripped {
        retry_after: Duration,
    },
    /// The bucket was already empty.
    Limited {
        retry_after: Duration,
    },
}
