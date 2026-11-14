//! Recording and fault-injecting decorators that delegate to the real hooks and the real account admin (the
//! allowed test decorators of AZ-3820). Every call lands in one shared log in call order.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use contracts::{
    AccountAdmin, AccountAdminError, AccountFilter, AccountInfo, AccountLifecycleHook,
    AccountProfile, AccountState, EraseProgress, LifecycleError, Page, Role, async_trait,
};
use sc_db::DbTx;
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// `remaining` is `None` for a failed call.
    Hook {
        position: usize,
        limit: u32,
        remaining: Option<u64>,
    },
    Finalize {
        ok: bool,
    },
}

#[derive(Clone, Default)]
pub struct Log(Arc<Mutex<Vec<Call>>>);

impl Log {
    pub fn record(&self, call: Call) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call);
    }

    pub fn calls(&self) -> Vec<Call> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// What a decorated hook does on its next call, once.
pub enum Fault {
    /// Fails before reaching the real hook.
    FailNext,
    /// Runs the real batch, then signals and never returns, so the test can drop the cycle mid-step.
    HangAfterNext(Arc<Notify>),
}

pub struct RecordedHook {
    position: usize,
    inner: Arc<dyn AccountLifecycleHook>,
    log: Log,
    armed: AtomicBool,
    fault: Option<Fault>,
}

#[async_trait]
impl AccountLifecycleHook for RecordedHook {
    async fn initialize(&self, tx: &mut DbTx, account: Uuid) -> Result<(), LifecycleError> {
        self.inner.initialize(tx, account).await
    }

    async fn erase_batch(
        &self,
        account: Uuid,
        limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        let fire = self.armed.swap(false, Ordering::SeqCst);
        if fire && matches!(self.fault, Some(Fault::FailNext)) {
            self.log.record(Call::Hook {
                position: self.position,
                limit,
                remaining: None,
            });
            return Err(LifecycleError::Unexpected(
                format!("injected failure at hook position {}", self.position).into(),
            ));
        }
        let result = self.inner.erase_batch(account, limit).await;
        self.log.record(Call::Hook {
            position: self.position,
            limit,
            remaining: result.as_ref().ok().map(|progress| progress.remaining),
        });
        if let (true, Some(Fault::HangAfterNext(entered))) = (fire, &self.fault) {
            entered.notify_one();
            std::future::pending::<()>().await;
        }
        result
    }
}

/// `hooks` wrapped in order; `fault` arms one position.
pub fn record_hooks(
    hooks: Vec<Arc<dyn AccountLifecycleHook>>,
    log: &Log,
    mut fault: Option<(usize, Fault)>,
) -> Vec<Arc<dyn AccountLifecycleHook>> {
    hooks
        .into_iter()
        .enumerate()
        .map(|(position, inner)| {
            let fault = fault
                .take_if(|(at, _)| *at == position)
                .map(|(_, fault)| fault);
            Arc::new(RecordedHook {
                position,
                inner,
                log: log.clone(),
                armed: AtomicBool::new(fault.is_some()),
                fault,
            }) as Arc<dyn AccountLifecycleHook>
        })
        .collect()
}

/// The real account admin with `finalize_deletion` recorded and, when armed, failed once.
pub struct RecordedAccounts {
    inner: Arc<dyn AccountAdmin>,
    log: Log,
    fail_finalize: AtomicBool,
}

impl RecordedAccounts {
    pub fn wrap(inner: Arc<dyn AccountAdmin>, log: &Log, fail_finalize: bool) -> Arc<Self> {
        Arc::new(RecordedAccounts {
            inner,
            log: log.clone(),
            fail_finalize: AtomicBool::new(fail_finalize),
        })
    }
}

#[async_trait]
impl AccountAdmin for RecordedAccounts {
    async fn list_accounts(
        &self,
        filter: AccountFilter,
        page: Page,
    ) -> Result<Vec<AccountInfo>, AccountAdminError> {
        self.inner.list_accounts(filter, page).await
    }

    async fn profile(&self, account: Uuid) -> Result<AccountProfile, AccountAdminError> {
        self.inner.profile(account).await
    }

    async fn lock_active_admins(&self, tx: &mut DbTx) -> Result<Vec<Uuid>, AccountAdminError> {
        self.inner.lock_active_admins(tx).await
    }

    async fn set_role(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        role: Role,
    ) -> Result<(), AccountAdminError> {
        self.inner.set_role(tx, account, role).await
    }

    async fn set_state(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        state: AccountState,
    ) -> Result<(), AccountAdminError> {
        self.inner.set_state(tx, account, state).await
    }

    async fn begin_deletion(&self, tx: &mut DbTx, account: Uuid) -> Result<(), AccountAdminError> {
        self.inner.begin_deletion(tx, account).await
    }

    async fn finalize_deletion(
        &self,
        tx: &mut DbTx,
        account: Uuid,
    ) -> Result<(), AccountAdminError> {
        if self.fail_finalize.swap(false, Ordering::SeqCst) {
            self.log.record(Call::Finalize { ok: false });
            return Err(AccountAdminError::Unexpected(
                "injected failure at finalize_deletion".into(),
            ));
        }
        let result = self.inner.finalize_deletion(tx, account).await;
        self.log.record(Call::Finalize { ok: result.is_ok() });
        result
    }

    async fn lock_deleting(&self, tx: &mut DbTx, account: Uuid) -> Result<bool, AccountAdminError> {
        self.inner.lock_deleting(tx, account).await
    }
}

/// The order rule of AC-1 over `calls`: every call of hook k > 0 comes after a 0 from hook k - 1 with no call of
/// hook k - 1 since; no limit exceeds `limit`; exactly one successful finalize, after which nothing is called,
/// and the call of every hook just before it reported 0. Returns the first breach.
pub fn order_breach(calls: &[Call], hooks: usize, limit: u32) -> Option<String> {
    let mut last: Vec<Option<Option<u64>>> = vec![None; hooks];
    let mut finalized = 0;
    for (index, call) in calls.iter().enumerate() {
        if finalized > 0 {
            return Some(format!("call {index} after the finalize: {call:?}"));
        }
        match *call {
            Call::Hook {
                position,
                limit: used,
                remaining,
            } => {
                if used > limit {
                    return Some(format!("call {index} with limit {used}"));
                }
                if position > 0 && last[position - 1] != Some(Some(0)) {
                    return Some(format!(
                        "call {index} of hook {position} before hook {} reported 0",
                        position - 1
                    ));
                }
                last[position] = Some(remaining);
            }
            Call::Finalize { ok } => {
                if let Some(position) = last.iter().position(|seen| *seen != Some(Some(0))) {
                    return Some(format!(
                        "finalize at call {index} before hook {position} reported 0"
                    ));
                }
                if ok {
                    finalized += 1;
                }
            }
        }
    }
    (finalized != 1).then(|| format!("{finalized} successful finalize calls"))
}

/// Calls that failed: a hook error or a failed finalize.
pub fn failed_calls(calls: &[Call]) -> usize {
    calls
        .iter()
        .filter(|call| {
            matches!(
                call,
                Call::Hook {
                    remaining: None,
                    ..
                } | Call::Finalize { ok: false }
            )
        })
        .count()
}
