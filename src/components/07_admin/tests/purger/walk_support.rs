//! The purger over scripted hook doubles and the real 01 account admin (claim and finalize) on a real database,
//! with a manual clock that only the doubles move.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use contracts::{AccountLifecycleHook, EraseProgress, LifecycleError, async_trait};
use sc_audit::AuditWriter;
use sc_clock::{Clock, ManualClock};
use sc_config::Environment;
use sc_db::{Db, DbTx};
use sc_testdb::{MIGRATOR, START, TestDb};
use sqlx::PgConnection;
use sunnycloud_admin::{AdminConfig, DeletionPurger};
use sunnycloud_identity::IdentityAccounts;
use uuid::Uuid;

use crate::purge_support::recording::{Call, Log};
use crate::purge_support::world::PURGER_KEYS;
use crate::support::seed;
use crate::support::settings::load_settings;

/// One scripted answer of a double: a remaining count, or a failure.
#[derive(Debug, Clone, Copy)]
pub enum Answer {
    Left(u64),
    Fail,
}

/// Answers from its script, then 0; each call moves the clock by `cost` and is logged.
pub struct Scripted {
    position: usize,
    script: Mutex<VecDeque<Answer>>,
    clock: Arc<ManualClock>,
    cost: Duration,
    log: Log,
}

#[async_trait]
impl AccountLifecycleHook for Scripted {
    async fn initialize(&self, _tx: &mut DbTx, _account: Uuid) -> Result<(), LifecycleError> {
        Err(LifecycleError::Unexpected(
            "not called by the purger".into(),
        ))
    }

    async fn erase_batch(
        &self,
        _account: Uuid,
        limit: u32,
    ) -> Result<EraseProgress, LifecycleError> {
        self.clock.tick(self.cost);
        let answer = self
            .script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or(Answer::Left(0));
        let remaining = match answer {
            Answer::Left(remaining) => Some(remaining),
            Answer::Fail => None,
        };
        self.log.record(Call::Hook {
            position: self.position,
            limit,
            remaining,
        });
        remaining
            .map(|remaining| EraseProgress { remaining })
            .ok_or_else(|| LifecycleError::Unexpected("scripted failure".into()))
    }
}

pub struct Walker {
    pub db: TestDb,
    pub api: Db,
    pub clock: Arc<ManualClock>,
    pub config: AdminConfig,
    pub log: Log,
}

impl Walker {
    pub async fn new() -> Walker {
        let settings = load_settings(Environment::Test, &PURGER_KEYS)
            .unwrap_or_else(|error| panic!("the test settings do not load: {error}"));
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        Walker {
            db,
            api,
            clock: sc_testdb::clock_at_start(),
            config: AdminConfig::from_settings(&settings),
            log: Log::default(),
        }
    }

    pub async fn fixture(&self) -> PgConnection {
        self.db.conn_as(MIGRATOR).await
    }

    /// A `deleting` account with no data, requested at the start time.
    pub async fn deleting_account(&self) -> Uuid {
        let account = Uuid::new_v4();
        let mut conn = self.fixture().await;
        seed::account(
            &mut conn,
            account,
            "Walk Fixture",
            "user",
            "deleting",
            START.as_unix_micros(),
        )
        .await;
        sqlx::query("UPDATE accounts SET deletion_requested_at = 'epoch'::timestamptz + $2::bigint * interval '1 microsecond' WHERE id = $1")
            .bind(account)
            .bind(START.as_unix_micros())
            .execute(&mut conn)
            .await
            .expect("set deletion_requested_at");
        account
    }

    /// One double per script, in order, each call costing `cost` on the clock.
    pub fn hooks(
        &self,
        scripts: Vec<Vec<Answer>>,
        cost: Duration,
    ) -> Vec<Arc<dyn AccountLifecycleHook>> {
        scripts
            .into_iter()
            .enumerate()
            .map(|(position, script)| {
                Arc::new(Scripted {
                    position,
                    script: Mutex::new(script.into()),
                    clock: self.clock.clone(),
                    cost,
                    log: self.log.clone(),
                }) as Arc<dyn AccountLifecycleHook>
            })
            .collect()
    }

    pub fn purger(&self, hooks: Vec<Arc<dyn AccountLifecycleHook>>) -> DeletionPurger {
        let clock: Arc<dyn Clock> = self.clock.clone();
        DeletionPurger::new(
            Arc::new(IdentityAccounts::new(self.api.clone(), clock.clone())),
            hooks,
            self.config.clone(),
            AuditWriter::new(self.api.clone(), clock.clone()),
            self.api.clone(),
            clock,
        )
    }

    pub fn hook_calls(&self) -> Vec<(usize, Option<u64>)> {
        self.log
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Hook {
                    position,
                    remaining,
                    ..
                } => Some((position, remaining)),
                Call::Finalize { .. } => None,
            })
            .collect()
    }

    pub async fn finish(self) {
        let Walker { db, api, .. } = self;
        drop(api);
        db.drop_db().await;
    }
}
