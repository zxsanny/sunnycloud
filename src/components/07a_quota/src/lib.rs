//! 07a quota: the per-account storage ledger. Depends on `contracts` and the helpers only.

mod config;
mod error;

pub use config::{MAX_LIMIT_BYTES, QuotaConfig};
pub use error::{db_error, ledger_error};
