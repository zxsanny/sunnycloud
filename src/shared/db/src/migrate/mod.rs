//! Reversible sqlx migrations embedded from `src/migrations/`, and the schema version check.
//!
//! [`Migrator::run_pending`] and [`Migrator::revert`] are for the one-shot migrate step (role
//! [`crate::DbRole::Migrator`]); [`Migrator::status`] and [`Migrator::check_version`] also work as the API
//! role, which `serve`, `/readyz` and `doctor` share. Contract: `_docs/02_document/contracts/shared_db/sc_db_migrations.md`.
//! Authoring rules: `README.md` of this crate.

mod error;
mod plan;
mod set;

pub use error::MigrationError;

use crate::db::Db;
use crate::error::DbError;
use plan::Plan;
pub(crate) use set::MigrationSet;

/// The versions a run applied or a revert undid, oldest first for `applied`, newest first for `reverted`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MigrationReport {
    pub applied: Vec<i64>,
    pub reverted: Vec<i64>,
}

/// What the bookkeeping table says compared with the files this binary embeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationStatus {
    pub applied: Vec<i64>,
    pub pending: Vec<i64>,
    /// The lowest version recorded as unsuccessful, if any.
    pub dirty: Option<i64>,
    /// Applied versions that have no file in this binary.
    pub unknown_newer: Vec<i64>,
    /// Applied versions whose file content differs from the embedded file.
    pub checksum_mismatch: Vec<i64>,
}

/// Whether the schema matches the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaState {
    Current,
    /// `n` embedded migrations are not applied yet.
    Pending {
        n: usize,
    },
    /// The database has an applied version this binary does not know.
    UnknownNewer,
}

impl SchemaState {
    pub(crate) fn from_plan(plan: &Plan) -> SchemaState {
        if !plan.unknown_newer.is_empty() {
            SchemaState::UnknownNewer
        } else if !plan.pending.is_empty() {
            SchemaState::Pending {
                n: plan.pending.len(),
            }
        } else {
            SchemaState::Current
        }
    }
}

/// The migrations embedded in this binary.
pub struct Migrator;

impl Migrator {
    /// Applies every pending migration, oldest first, each in its own transaction under sqlx's advisory
    /// lock. A failure leaves the earlier ones applied. Refuses a `Db` that is not the migrator role.
    pub async fn run_pending(db: &Db) -> Result<MigrationReport, MigrationError> {
        MigrationSet::embedded().run_pending(db).await
    }

    /// Reverts the newest `steps` applied migrations with their `down` files, newest first.
    pub async fn revert(db: &Db, steps: u32) -> Result<MigrationReport, MigrationError> {
        MigrationSet::embedded().revert(db, steps).await
    }

    /// Read-only; works as either role. A database without the bookkeeping table has nothing applied.
    pub async fn status(db: &Db) -> Result<MigrationStatus, DbError> {
        MigrationSet::embedded().status(db).await
    }

    /// Read-only; works as either role. `serve` refuses to start unless this is `Ok(Current)`. An applied file
    /// whose content changed is [`MigrationError::ChecksumMismatch`] and an unsuccessful row is
    /// [`MigrationError::Dirty`].
    pub async fn check_version(db: &Db) -> Result<SchemaState, MigrationError> {
        MigrationSet::embedded().check_version(db).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(pending: &[i64], unknown_newer: &[i64]) -> Plan {
        Plan {
            pending: pending.to_vec(),
            unknown_newer: unknown_newer.to_vec(),
            checksum_mismatch: Vec::new(),
        }
    }

    #[test]
    fn no_difference_is_current() {
        assert_eq!(
            SchemaState::from_plan(&plan(&[], &[])),
            SchemaState::Current
        );
    }

    #[test]
    fn missing_versions_are_pending_with_their_count() {
        assert_eq!(
            SchemaState::from_plan(&plan(&[3, 4], &[])),
            SchemaState::Pending { n: 2 }
        );
    }

    #[test]
    fn an_unknown_applied_version_wins_over_pending() {
        assert_eq!(
            SchemaState::from_plan(&plan(&[3], &[9])),
            SchemaState::UnknownNewer
        );
    }
}
