use contracts::{QuotaLedger, QuotaLedgerError, Reserved, async_trait};
use sc_db::DbTx;
use sc_observability::log_warn;
use uuid::Uuid;

use crate::config::QuotaConfig;
use crate::error::{LedgerFault, ledger_error};
use crate::ledger_repo;

/// The ledger over the `quota` table. It keeps no state besides the installation default limit, and it never
/// opens, commits or rolls back a transaction: everything it changes rolls back with the caller's transaction
/// or savepoint. A successful `reserve` holds the account's row lock until the caller's transaction ends.
#[derive(Debug, Clone)]
pub struct PgQuotaLedger {
    default_quota_bytes: i64,
}

impl PgQuotaLedger {
    pub fn new(config: QuotaConfig) -> Self {
        PgQuotaLedger {
            default_quota_bytes: config.default_quota_bytes,
        }
    }
}

fn require_non_negative(bytes: i64) -> Result<(), QuotaLedgerError> {
    if bytes < 0 {
        return Err(QuotaLedgerError::InvariantViolation);
    }
    Ok(())
}

fn missing_row(account: Uuid) -> QuotaLedgerError {
    QuotaLedgerError::Unexpected(Box::new(LedgerFault::MissingRow { account }))
}

#[async_trait]
impl QuotaLedger for PgQuotaLedger {
    async fn reserve(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        bytes: i64,
    ) -> Result<Reserved, QuotaLedgerError> {
        require_non_negative(bytes)?;
        let granted = ledger_repo::reserve(tx, account, bytes, self.default_quota_bytes)
            .await
            .map_err(ledger_error)?;
        if let Some(reserved_bytes) = granted {
            return Ok(Reserved { reserved_bytes });
        }
        let refused = ledger_repo::counters(tx, account, self.default_quota_bytes)
            .await
            .map_err(ledger_error)?
            .ok_or_else(|| missing_row(account))?;
        Err(QuotaLedgerError::QuotaExceeded {
            limit: refused.limit,
            used: refused.used,
            reserved: refused.reserved,
        })
    }

    async fn release_reservation(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        bytes: i64,
    ) -> Result<(), QuotaLedgerError> {
        require_non_negative(bytes)?;
        let reserved = ledger_repo::lock_reserved(tx, account)
            .await
            .map_err(ledger_error)?
            .ok_or_else(|| missing_row(account))?;
        ledger_repo::release(tx, account, bytes)
            .await
            .map_err(ledger_error)?;
        if bytes > reserved {
            log_warn!("quota release clamped at zero", account_id = account);
        }
        Ok(())
    }

    async fn adjust_used(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        delta: i64,
    ) -> Result<(), QuotaLedgerError> {
        if ledger_repo::adjust(tx, account, delta)
            .await
            .map_err(ledger_error)?
        {
            return Ok(());
        }
        if ledger_repo::exists(tx, account)
            .await
            .map_err(ledger_error)?
        {
            Err(QuotaLedgerError::InvariantViolation)
        } else {
            Err(missing_row(account))
        }
    }
}
