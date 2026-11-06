//! The two health ports, declared here so `contracts` can re-export them without this crate importing it.

use std::future::Future;
use std::pin::Pin;

use serde::Serialize;

/// Boxed future of a check, so the traits stay object safe (same shape as `sc_clock::SleepFuture`).
pub type CheckFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
    Degraded,
    Down,
}

/// Result of one [`HealthContributor`] check. `detail` never holds a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HealthItem {
    pub status: HealthStatus,
    pub detail: String,
}

impl HealthItem {
    pub fn new(status: HealthStatus, detail: impl Into<String>) -> Self {
        HealthItem {
            status,
            detail: detail.into(),
        }
    }

    pub fn ok() -> Self {
        Self::new(HealthStatus::Ok, "")
    }
}

/// A component's contribution to the admin health view (07). The caller bounds each check with a timeout.
pub trait HealthContributor: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self) -> CheckFuture<'_, HealthItem>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadinessStatus {
    Ok,
    Degraded,
    Failing,
}

/// Result of one [`ReadinessCheck`]: the item of the `/readyz` body. `detail` never holds a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadinessItem {
    pub status: ReadinessStatus,
    pub detail: String,
}

impl ReadinessItem {
    pub fn new(status: ReadinessStatus, detail: impl Into<String>) -> Self {
        ReadinessItem {
            status,
            detail: detail.into(),
        }
    }

    pub fn ok() -> Self {
        Self::new(ReadinessStatus::Ok, "")
    }

    pub fn failing(detail: impl Into<String>) -> Self {
        Self::new(ReadinessStatus::Failing, detail)
    }
}

/// One condition of `/readyz`. Registered in a [`crate::ReadinessRegistry`]; `/readyz` answers 200 only when
/// every registered check returns `Ok`.
pub trait ReadinessCheck: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self) -> CheckFuture<'_, ReadinessItem>;
}
