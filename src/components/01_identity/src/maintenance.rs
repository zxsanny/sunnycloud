use std::time::Duration;

use contracts::{CycleStats, Fault, Janitor, JanitorError, async_trait};
use sc_clock::Timestamp;
use sc_db::Db;

use crate::auth_repo;
use crate::error::IdentityError;
use crate::janitor_repo::Target;

/// Rows deleted per transaction; a cycle repeats until a batch comes back short.
const BATCH: i64 = 1000;
/// How long a device grant lingers after it expired or was consumed.
const GRANT_RETENTION: Duration = Duration::from_secs(60 * 60);
/// How long a session lingers after it was revoked or its refresh token expired.
const SESSION_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Rows removed by one cycle, per table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Swept {
    pub oauth_states: u64,
    pub login_codes: u64,
    pub device_grants: u64,
    pub sessions: u64,
}

impl Swept {
    pub fn total(&self) -> u64 {
        self.oauth_states + self.login_codes + self.device_grants + self.sessions
    }
}

/// Removes expired identity rows: sign-in states and login codes at their expiry, device grants one hour after
/// they expired, sessions 30 days after they were revoked or their refresh token expired. Each batch is one
/// short transaction in system scope; nothing live is touched.
pub struct IdentityMaintenance {
    db: Db,
}

impl IdentityMaintenance {
    pub fn new(db: Db) -> Self {
        IdentityMaintenance { db }
    }

    /// One cycle at `now`, with the deleted rows counted per table.
    pub async fn sweep(&self, now: Timestamp) -> Result<Swept, IdentityError> {
        Ok(Swept {
            oauth_states: self.drain(Target::OauthStates, now).await?,
            login_codes: self.drain(Target::LoginCodes, now).await?,
            device_grants: self
                .drain(Target::DeviceGrants, earlier(now, GRANT_RETENTION)?)
                .await?,
            sessions: self
                .drain(Target::Sessions, earlier(now, SESSION_RETENTION)?)
                .await?,
        })
    }

    async fn drain(&self, target: Target, cutoff: Timestamp) -> Result<u64, IdentityError> {
        let mut total = 0;
        loop {
            let deleted = auth_repo::sweep_batch(&self.db, target, cutoff, BATCH)
                .await
                .map_err(IdentityError::Database)?;
            total += deleted;
            if deleted < BATCH.unsigned_abs() {
                return Ok(total);
            }
        }
    }
}

fn earlier(at: Timestamp, by: Duration) -> Result<Timestamp, IdentityError> {
    at.checked_sub(by).ok_or(IdentityError::TimeOutOfRange)
}

#[async_trait]
impl Janitor for IdentityMaintenance {
    fn name(&self) -> &'static str {
        "identity"
    }

    async fn run_cycle(&self, now: Timestamp) -> Result<CycleStats, JanitorError> {
        let swept = self
            .sweep(now)
            .await
            .map_err(|error| JanitorError::Unexpected(Box::new(error) as Fault))?;
        Ok(CycleStats {
            processed: swept.total(),
        })
    }
}
