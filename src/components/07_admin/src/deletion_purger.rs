//! `DeletionPurger`: erases the accounts in `deleting` across the components by driving their erase hooks, then
//! finalizes each in 01 with its `account_deleted` audit row.
//!
//! A cycle lists the `deleting` accounts and walks each through the hooks in the order given. Every step is its
//! own short claim transaction (`purge_step`), so the purger keeps no position in memory or in a table: a crash,
//! a failed step or a dropped cycle is resumed by the next cycle from the first hook, and the hooks report 0
//! quickly for what is already gone.

use std::sync::Arc;
use std::time::Duration;

use contracts::{
    AccountAdmin, AccountFilter, AccountInfo, AccountLifecycleHook, AccountStatus, CycleStats,
    Janitor, JanitorError, Page, async_trait,
};
use sc_audit::AuditWriter;
use sc_clock::{Clock, MonotonicInstant, Timestamp};
use sc_db::Db;
use sc_observability::{Histogram, Metrics, NoLabels, ObsError, log_error, log_info, log_warn};
use uuid::Uuid;

use crate::AdminConfig;
use crate::purge_step::{Claimed, StepAt, StepError, StepFailure, erase_step, finalize_step};

const NAME: &str = "deletion_purger";
const DELETION_SECONDS: &str = "account_deletion_seconds";

/// How one account's walk in this cycle ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Walk {
    Finalized,
    /// Another purger holds the claim, or the account left `deleting`; the next cycle looks again.
    Skipped,
    /// A step failed and was logged; the next cycle retries.
    Failed,
    BudgetSpent,
}

/// The deletion purger. The hook list is the call order: it is kept as given and never sorted.
pub struct DeletionPurger {
    accounts: Arc<dyn AccountAdmin>,
    hooks: Vec<Arc<dyn AccountLifecycleHook>>,
    config: AdminConfig,
    audit: AuditWriter,
    db: Db,
    clock: Arc<dyn Clock>,
    deletion_seconds: Option<Histogram<NoLabels>>,
}

impl DeletionPurger {
    /// `db` runs the claim transactions; the cycle budget is measured on `clock`'s monotonic time, and the
    /// deletion time from the request to the finalize on its wall time.
    pub fn new(
        accounts: Arc<dyn AccountAdmin>,
        hooks: Vec<Arc<dyn AccountLifecycleHook>>,
        config: AdminConfig,
        audit: AuditWriter,
        db: Db,
        clock: Arc<dyn Clock>,
    ) -> Self {
        DeletionPurger {
            accounts,
            hooks,
            config,
            audit,
            db,
            clock,
            deletion_seconds: None,
        }
    }

    /// Registers `account_deletion_seconds`; `Duplicate` when already registered. A process without a registry
    /// leaves it out and records nothing.
    pub fn with_metrics(mut self, metrics: &Metrics) -> Result<Self, ObsError> {
        self.deletion_seconds = Some(metrics.histogram(DELETION_SECONDS)?);
        Ok(self)
    }

    pub fn accounts(&self) -> &Arc<dyn AccountAdmin> {
        &self.accounts
    }

    pub fn hooks(&self) -> &[Arc<dyn AccountLifecycleHook>] {
        &self.hooks
    }

    pub fn config(&self) -> &AdminConfig {
        &self.config
    }

    /// The batch limit and the listing page. `AdminConfig::validate` refuses a size above `u32::MAX` at start;
    /// the clamp keeps every limit at or below the configured size regardless.
    fn limit(&self) -> u32 {
        u32::try_from(self.config.account_delete_batch_size).unwrap_or(u32::MAX)
    }

    fn spent(&self, started: MonotonicInstant) -> bool {
        self.clock.monotonic().saturating_duration_since(started)
            >= self.config.account_delete_cycle_budget
    }

    /// Every `deleting` account, oldest first, page by page.
    async fn deleting_accounts(&self) -> Result<Vec<AccountInfo>, JanitorError> {
        let limit = self.limit();
        let filter = AccountFilter {
            role: None,
            state: Some(AccountStatus::Deleting),
        };
        let mut accounts = Vec::new();
        loop {
            let page = Page {
                offset: accounts.len() as u64,
                limit,
            };
            let listed = self
                .accounts
                .list_accounts(filter, page)
                .await
                .map_err(|error| JanitorError::Unexpected(Box::new(error)))?;
            let last = listed.len() < limit as usize;
            accounts.extend(listed);
            if last {
                return Ok(accounts);
            }
        }
    }

    fn alert_if_late(&self, account: &AccountInfo, now: Timestamp) {
        let Some(requested) = account.deletion_requested_at else {
            log_warn!(
                "deleting account has no deletion request time",
                account_id = account.id
            );
            return;
        };
        let late = requested
            .checked_add(self.config.account_delete_alert_after)
            .is_none_or(|alert_at| alert_at <= now);
        if late {
            log_warn!(
                "account deletion older than account_delete_alert_after",
                account_id = account.id
            );
        }
    }

    /// Passes over the hooks, then the finalize. Within a pass, hook N + 1 is called only after hook N reported 0.
    /// The finalize follows the first pass, other than the walk's first, in which every hook reported 0 at its
    /// first call. A later hook can leave work an earlier one counts (03 queues the blobs of open uploads, which
    /// 02 counts until 04's GC removed them), and a hook can report 0 in the call that erased its last rows; so
    /// only a pass that follows a full pass and finds nothing shows that every hook is done.
    async fn walk(&self, account: Uuid, started: MonotonicInstant) -> Walk {
        let limit = self.limit();
        let mut first_pass = true;
        loop {
            let mut clean = true;
            for (position, hook) in self.hooks.iter().enumerate() {
                loop {
                    if self.spent(started) {
                        return Walk::BudgetSpent;
                    }
                    let step = erase_step(
                        &self.db,
                        self.accounts.as_ref(),
                        hook.as_ref(),
                        account,
                        limit,
                    )
                    .await;
                    match step {
                        Ok(Claimed::Done(progress)) if progress.remaining == 0 => break,
                        Ok(Claimed::Done(_)) => clean = false,
                        Ok(Claimed::Skipped) => return Walk::Skipped,
                        Err(cause) => return failed(account, StepAt::Hook(position), cause),
                    }
                }
            }
            if clean && !first_pass {
                break;
            }
            first_pass = false;
        }
        if self.spent(started) {
            return Walk::BudgetSpent;
        }
        match finalize_step(&self.db, self.accounts.as_ref(), &self.audit, account).await {
            Ok(Claimed::Done(())) => Walk::Finalized,
            Ok(Claimed::Skipped) => Walk::Skipped,
            Err(cause) => failed(account, StepAt::Finalize(self.hooks.len()), cause),
        }
    }

    fn finalized(&self, account: &AccountInfo) {
        log_info!("account deleted", account_id = account.id);
        let (Some(histogram), Some(requested)) =
            (&self.deletion_seconds, account.deletion_requested_at)
        else {
            return;
        };
        // A request time ahead of this clock counts as no time at all.
        let took = self
            .clock
            .now_utc()
            .checked_duration_since(requested)
            .unwrap_or(Duration::ZERO);
        histogram.observe(NoLabels, took.as_secs_f64());
    }
}

fn failed(account: Uuid, at: StepAt, cause: StepError) -> Walk {
    let failure = StepFailure { at, cause };
    log_error!(
        "account erase step failed",
        account_id = account,
        err = &failure
    );
    Walk::Failed
}

#[async_trait]
impl Janitor for DeletionPurger {
    fn name(&self) -> &'static str {
        NAME
    }

    /// Lists the `deleting` accounts and walks them oldest first until `account_delete_cycle_budget` is spent
    /// (checked before every step). A failed step is logged at ERROR and retried next cycle, not returned; only a
    /// failed listing is. `processed` counts the accounts finalized in this cycle.
    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError> {
        let started = self.clock.monotonic();
        let deleting = self.deleting_accounts().await?;
        let mut processed = 0;
        for account in &deleting {
            self.alert_if_late(account, now);
        }
        for account in &deleting {
            match self.walk(account.id, started).await {
                Walk::Finalized => {
                    processed += 1;
                    self.finalized(account);
                }
                Walk::Skipped | Walk::Failed => {}
                Walk::BudgetSpent => break,
            }
        }
        Ok(CycleStats { processed })
    }
}
