//! Ports of 07a quota.

use std::collections::HashMap;

use async_trait::async_trait;
use sc_db::DbTx;
use thiserror::Error;
use uuid::Uuid;

use crate::common::Fault;

/// Ledger state of one account, in bytes. `limit_bytes` is `None` when the installation default applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaView {
    pub limit_bytes: Option<i64>,
    pub effective_limit: i64,
    pub used: i64,
    pub reserved: i64,
}

/// Outcome of a granted reservation: the account's reserved total after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reserved {
    pub reserved_bytes: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drift {
    pub stored: i64,
    pub computed: i64,
}

#[derive(Debug, Error)]
pub enum QuotaLedgerError {
    #[error("quota exceeded")]
    QuotaExceeded {
        limit: i64,
        used: i64,
        reserved: i64,
    },
    /// `adjust_used` would make the used total negative.
    #[error("quota invariant violated")]
    InvariantViolation,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// The ledger as 02 and 03 use it, inside their own transaction. It never opens one.
#[async_trait]
pub trait QuotaLedger: Send + Sync {
    async fn reserve(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        bytes: i64,
    ) -> Result<Reserved, QuotaLedgerError>;

    /// Clamped at zero.
    async fn release_reservation(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        bytes: i64,
    ) -> Result<(), QuotaLedgerError>;

    async fn adjust_used(
        &self,
        tx: &mut DbTx,
        account: Uuid,
        delta: i64,
    ) -> Result<(), QuotaLedgerError>;
}

#[derive(Debug, Error)]
pub enum QuotaAdminError {
    #[error("not found")]
    NotFound,
    /// A limit is valid from zero up to 2^53 bytes.
    #[error("invalid limit")]
    InvalidLimit,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

#[async_trait]
pub trait QuotaAdmin: Send + Sync {
    async fn view(&self, account: Uuid) -> Result<QuotaView, QuotaAdminError>;

    /// Unknown accounts are omitted.
    async fn views(&self, accounts: &[Uuid]) -> Result<HashMap<Uuid, QuotaView>, QuotaAdminError>;

    /// `None` restores the installation default.
    async fn set_limit(&self, account: Uuid, limit: Option<i64>) -> Result<(), QuotaAdminError>;

    /// Compares the stored usage with the numbers the caller computed; repairs it when `repair` is set.
    async fn recompute(
        &self,
        account: Uuid,
        live_bytes: i64,
        reserved_bytes: i64,
        repair: bool,
    ) -> Result<Drift, QuotaAdminError>;
}
