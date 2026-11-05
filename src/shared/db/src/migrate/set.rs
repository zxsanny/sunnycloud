use sqlx::migrate::{AppliedMigration, Migrate, MigrateError, Migration};
use sqlx::{PgConnection, Postgres};

use super::plan::{self, Plan};
use super::{MigrationError, MigrationReport, MigrationStatus, SchemaState};
use crate::config::DbRole;
use crate::db::Db;
use crate::error::DbError;
use crate::stats::Stats;

const TABLE: &str = "_sqlx_migrations";

static EMBEDDED: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

struct Snapshot {
    applied: Vec<AppliedMigration>,
    dirty: Option<i64>,
}

/// The files a run compares the database with: the embedded ones, or (in tests) a seeded set.
pub(crate) struct MigrationSet {
    migrations: Vec<Migration>,
}

impl MigrationSet {
    pub(crate) fn embedded() -> Self {
        MigrationSet::from_migrations(EMBEDDED.iter().cloned().collect())
    }

    pub(crate) fn from_migrations(mut migrations: Vec<Migration>) -> Self {
        migrations.sort();
        MigrationSet { migrations }
    }

    fn ups(&self) -> Vec<&Migration> {
        self.migrations
            .iter()
            .filter(|migration| migration.migration_type.is_up_migration())
            .collect()
    }

    fn down(&self, version: i64) -> Option<&Migration> {
        self.migrations.iter().find(|migration| {
            migration.migration_type.is_down_migration() && migration.version == version
        })
    }

    fn plan(&self, snapshot: &Snapshot) -> Plan {
        plan::diff(&self.ups(), &snapshot.applied)
    }

    pub(crate) async fn status(&self, db: &Db) -> Result<MigrationStatus, DbError> {
        let mut conn = db.pool.acquire().await?;
        self.status_on(&db.stats, &mut conn).await
    }

    /// Reads on a connection the caller holds, so `Db::health` probes and counts on one connection.
    pub(crate) async fn status_on(
        &self,
        stats: &Stats,
        conn: &mut PgConnection,
    ) -> Result<MigrationStatus, DbError> {
        let snapshot = snapshot(conn).await?;
        let plan = self.plan(&snapshot);
        stats.set_pending(plan.pending.len());
        Ok(MigrationStatus {
            applied: snapshot.applied.iter().map(|row| row.version).collect(),
            pending: plan.pending,
            dirty: snapshot.dirty,
            unknown_newer: plan.unknown_newer,
            checksum_mismatch: plan.checksum_mismatch,
        })
    }

    pub(crate) async fn check_version(&self, db: &Db) -> Result<SchemaState, MigrationError> {
        let mut conn = db.pool.acquire().await.map_err(DbError::from)?;
        let snapshot = snapshot(&mut conn).await?;
        let plan = self.plan(&snapshot);
        db.stats.set_pending(plan.pending.len());
        if let Some(version) = snapshot.dirty {
            return Err(MigrationError::Dirty { version });
        }
        if plan.unknown_newer.is_empty()
            && let Some(&version) = plan.checksum_mismatch.first()
        {
            return Err(MigrationError::ChecksumMismatch { version });
        }
        Ok(SchemaState::from_plan(&plan))
    }

    pub(crate) async fn run_pending(&self, db: &Db) -> Result<MigrationReport, MigrationError> {
        require_migrator(db)?;
        let report = locked(db, async |conn| {
            ensure_table(conn).await?;
            let snapshot = snapshot(conn).await?;
            let plan = self.plan(&snapshot);
            refuse_unusable(&snapshot, &plan)?;
            if let Some(&version) = plan.checksum_mismatch.first() {
                return Err(MigrationError::ChecksumMismatch { version });
            }
            let mut report = MigrationReport::default();
            for migration in self.ups() {
                if plan.pending.contains(&migration.version) {
                    conn.apply(TABLE, migration)
                        .await
                        .map_err(|error| MigrationError::from_migrate(error, false))?;
                    report.applied.push(migration.version);
                }
            }
            Ok(report)
        })
        .await?;
        db.stats.set_pending(0);
        Ok(report)
    }

    pub(crate) async fn revert(
        &self,
        db: &Db,
        steps: u32,
    ) -> Result<MigrationReport, MigrationError> {
        require_migrator(db)?;
        locked(db, async |conn| {
            ensure_table(conn).await?;
            let snapshot = snapshot(conn).await?;
            refuse_unusable(&snapshot, &self.plan(&snapshot))?;
            let mut newest_first: Vec<i64> =
                snapshot.applied.iter().map(|row| row.version).collect();
            newest_first.reverse();
            let mut report = MigrationReport::default();
            for version in newest_first.into_iter().take(steps as usize) {
                let down = self
                    .down(version)
                    .ok_or(MigrationError::IrreversibleMigration { version })?;
                conn.revert(TABLE, down)
                    .await
                    .map_err(|error| MigrationError::from_migrate(error, true))?;
                report.reverted.push(version);
            }
            Ok(report)
        })
        .await
    }
}

fn require_migrator(db: &Db) -> Result<(), MigrationError> {
    if db.role == DbRole::Migrator {
        Ok(())
    } else {
        Err(MigrationError::WrongRole)
    }
}

/// A dirty row or a version this binary does not know blocks both directions.
fn refuse_unusable(snapshot: &Snapshot, plan: &Plan) -> Result<(), MigrationError> {
    if let Some(version) = snapshot.dirty {
        return Err(MigrationError::Dirty { version });
    }
    if let Some(&version) = plan.unknown_newer.last() {
        return Err(MigrationError::UnknownNewer { version });
    }
    Ok(())
}

async fn ensure_table(conn: &mut PgConnection) -> Result<(), MigrationError> {
    conn.ensure_migrations_table(TABLE)
        .await
        .map_err(|error| MigrationError::from_migrate(error, false))
}

/// Runs `f` on one connection holding sqlx's session advisory lock. A failure closes the connection instead
/// of returning it to the pool, because the session still holds the lock.
async fn locked<T>(
    db: &Db,
    f: impl AsyncFnOnce(&mut PgConnection) -> Result<T, MigrationError>,
) -> Result<T, MigrationError> {
    let mut conn = db.pool.acquire().await.map_err(DbError::from)?;
    if let Err(error) = conn.lock().await {
        conn.close_on_drop();
        return Err(MigrationError::from_migrate(error, false));
    }
    match f(&mut conn).await {
        Ok(value) => {
            if let Err(error) = conn.unlock().await {
                conn.close_on_drop();
                return Err(MigrationError::from_migrate(error, false));
            }
            Ok(value)
        }
        Err(error) => {
            conn.close_on_drop();
            Err(error)
        }
    }
}

/// The bookkeeping rows, read without creating anything: a database that was never migrated has none.
async fn snapshot(conn: &mut PgConnection) -> Result<Snapshot, DbError> {
    let exists: bool =
        sqlx::query_scalar::<Postgres, bool>("SELECT to_regclass($1::text) IS NOT NULL")
            .bind(TABLE)
            .fetch_one(&mut *conn)
            .await?;
    if !exists {
        return Ok(Snapshot {
            applied: Vec::new(),
            dirty: None,
        });
    }
    let dirty = conn.dirty_version(TABLE).await.map_err(db_error)?;
    let applied = conn
        .list_applied_migrations(TABLE)
        .await
        .map_err(db_error)?;
    Ok(Snapshot { applied, dirty })
}

fn db_error(error: MigrateError) -> DbError {
    match error {
        MigrateError::Execute(error) => error.into(),
        other => DbError::Other(other.to_string()),
    }
}

#[cfg(test)]
impl MigrationSet {
    /// The set with `extra` files added: how the tests stand in for "a binary with one more migration".
    pub(crate) fn plus(mut self, extra: impl IntoIterator<Item = Migration>) -> Self {
        self.migrations.extend(extra);
        MigrationSet::from_migrations(self.migrations)
    }

    /// The set with the `up` file of `version` replaced: "the file content was changed afterwards".
    pub(crate) fn with_up_sql(self, version: i64, sql: &str) -> Self {
        let migrations = self
            .migrations
            .into_iter()
            .map(|migration| {
                if migration.version == version && migration.migration_type.is_up_migration() {
                    crate::tests::seeded::migration(version, migration.migration_type, sql)
                } else {
                    migration
                }
            })
            .collect();
        MigrationSet::from_migrations(migrations)
    }
}
