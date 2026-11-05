//! The only code that owns PostgreSQL connections, transaction shape and tenant scope.
//!
//! Components never open a pool, set a session variable or keep a transaction across I/O. Contract:
//! `_docs/02_document/contracts/shared_db/sc_db_access.md`.
//!
//! - [`Db::connect`] opens a pool for [`DbRole::Api`] (`sunnycloud`) or [`DbRole::Migrator`]
//!   (`sunnycloud_migrator`) and refuses a connection under the wrong role.
//! - [`Db::run`] and [`Db::run_system`] run a closure in one short transaction whose first statement sets
//!   the scope; an unset scope sees no rows under the row policies.
//! - [`DbTx`] is the open transaction: [`DbTx::savepoint`], [`DbTx::advisory_lock_account`].
//! - [`DbError`] is the SQLSTATE mapping; [`RunError`] pairs it with the closure's own error.
//!
//! The closure awaits only the database: a held long-poll or an HTTP call inside a transaction is a review
//! finding, backed by `db_idle_in_tx_timeout`.

mod config;
mod db;
mod error;
mod roles;
mod scope;
mod tx;

#[cfg(test)]
mod tests;

pub use config::{DbConfig, DbRole};
pub use db::Db;
pub use error::{DbError, RunError};
pub use scope::{AccountId, Scope};
pub use tx::DbTx;
