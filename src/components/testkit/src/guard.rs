//! The `AccountGuard` port backed at its boundary, for suites that wire `PgTreeService` without the identity
//! component (the real guard is wired by E-13/c16).

use std::sync::{Mutex, PoisonError};

use contracts::{AccountGuard, AccountGuardError, async_trait};
use sc_db::DbTx;
use uuid::Uuid;

use crate::account_repo;

/// Reads the account state `FOR SHARE` as the identity guard does, and records whether the transaction had
/// written or locked anything before it ran.
#[derive(Default)]
pub struct StateGuard {
    first_statement: Mutex<Vec<bool>>,
}

impl StateGuard {
    /// One entry per call: `true` when no row had been written or locked in the transaction before the guard.
    pub fn calls(&self) -> Vec<bool> {
        self.first_statement
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl AccountGuard for StateGuard {
    async fn ensure_active(&self, tx: &mut DbTx, account: Uuid) -> Result<(), AccountGuardError> {
        let unexpected = |error: sqlx::Error| AccountGuardError::Unexpected(Box::new(error));
        let clean = account_repo::nothing_written_yet(tx)
            .await
            .map_err(unexpected)?;
        self.first_statement
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(clean);
        let state = account_repo::state_for_share(tx, account)
            .await
            .map_err(unexpected)?;
        match state.as_deref() {
            Some("active") => Ok(()),
            _ => Err(AccountGuardError::AccountNotActive),
        }
    }
}
