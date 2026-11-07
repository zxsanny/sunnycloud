//! 07a quota: the per-account storage ledger. Depends on `contracts` and the helpers only.

mod admin;
mod admin_repo;
mod config;
mod error;
mod ledger;
mod ledger_repo;

pub use admin::{CounterDrift, PgQuotaAdmin};
pub use config::{MAX_LIMIT_BYTES, QuotaConfig};
pub use error::{LedgerFault, admin_error, db_error, ledger_error, lifecycle_error};
pub use ledger::PgQuotaLedger;
