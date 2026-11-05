use std::time::Duration;

use sc_config::{Secret, Settings};

use crate::error::DbError;

/// The two application roles of ADR-014.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbRole {
    /// Role `sunnycloud`: DML only, subject to row-level security.
    Api,
    /// Role `sunnycloud_migrator`: owns the schema, the only role with DDL.
    Migrator,
}

impl DbRole {
    pub(crate) fn user(self) -> &'static str {
        match self {
            DbRole::Api => "sunnycloud",
            DbRole::Migrator => "sunnycloud_migrator",
        }
    }
}

/// The Database group of `sc_config`, field names equal to the config keys.
#[derive(Debug, Clone)]
pub struct DbConfig {
    pub db_api_url: Secret<String>,
    pub db_migrator_url: Option<Secret<String>>,
    pub db_pool_max: u32,
    pub db_acquire_timeout: Duration,
    pub db_idle_in_tx_timeout: Duration,
    pub db_statement_timeout: Duration,
    /// How long the pool may take to serve a `run` after the database returns. `Db` keeps no state for it:
    /// the pool reconnects on demand, and the recovery test holds the pool to this bound.
    pub db_recovery_budget: Duration,
}

impl DbConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        DbConfig {
            db_api_url: settings.db_api_url.clone(),
            db_migrator_url: settings.db_migrator_url.clone(),
            db_pool_max: u32::try_from(settings.db_pool_max)
                .unwrap_or(u32::MAX)
                .max(1),
            db_acquire_timeout: settings.db_acquire_timeout.get(),
            db_idle_in_tx_timeout: settings.db_idle_in_tx_timeout.get(),
            db_statement_timeout: settings.db_statement_timeout.get(),
            db_recovery_budget: settings.db_recovery_budget.get(),
        }
    }

    pub(crate) fn url(&self, role: DbRole) -> Result<&str, DbError> {
        match role {
            DbRole::Api => Ok(self.db_api_url.expose()),
            DbRole::Migrator => self
                .db_migrator_url
                .as_ref()
                .map(|url| url.expose().as_str())
                .ok_or_else(|| DbError::Other("db_migrator_url is not set".to_string())),
        }
    }
}
