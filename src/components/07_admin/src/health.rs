use std::time::Duration;

use contracts::{Actor, HealthContributor, HealthItem, HealthReport, HealthStatus};
use sc_observability::log_error;
use tokio::task::JoinSet;

use crate::actor::authorize;
use crate::admin_service::AdminServiceImpl;
use crate::error::AdminCallError;

/// Bound of one check when `health_check_timeout` is unset: neither the key table nor 07 gives a default, and
/// the page must not wait on a hung check. Same value as the `/readyz` check bound of `sc_observability`.
const FALLBACK_CHECK_TIMEOUT: Duration = Duration::from_secs(2);

const TIMED_OUT: &str = "check timed out";
const FAILED: &str = "check failed";

impl AdminServiceImpl {
    /// Every registered contributor, in registration order, each run as its own task under
    /// `health_check_timeout`, so the call lasts as long as the slowest bounded check. A check that times out
    /// or panics is reported `down`; it never hides the others. Stateless: every call runs every check.
    pub async fn health(&self, actor: &Actor) -> Result<HealthReport, AdminCallError> {
        authorize(actor)?;
        let timeout = self
            .config
            .health_check_timeout
            .unwrap_or(FALLBACK_CHECK_TIMEOUT);
        let names: Vec<String> = self.health.iter().map(|c| c.name().to_owned()).collect();
        let mut items = vec![HealthItem::new(HealthStatus::Down, FAILED); names.len()];
        let mut running = JoinSet::new();
        for (position, contributor) in self.health.iter().enumerate() {
            let contributor = contributor.clone();
            running.spawn(async move { (position, bounded(contributor.as_ref(), timeout).await) });
        }
        // A task that panicked delivers nothing and keeps the `FAILED` item of its position; the cause is logged,
        // never copied into the item, since it could carry a secret.
        while let Some(finished) = running.join_next().await {
            match finished {
                Ok((position, item)) => items[position] = item,
                Err(error) => log_error!("health check task failed", err = &error),
            }
        }
        let items = names.into_iter().zip(items).collect();
        Ok(HealthReport { items })
    }
}

async fn bounded(contributor: &dyn HealthContributor, timeout: Duration) -> HealthItem {
    tokio::time::timeout(timeout, contributor.check())
        .await
        .unwrap_or_else(|_| HealthItem::new(HealthStatus::Down, TIMED_OUT))
}
