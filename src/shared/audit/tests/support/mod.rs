//! The audit tests' fixture on the shared `sc_testdb` harness: a migrated clone, a writer on the API role and a
//! clock that stands at [`START`] until ticked.

#![allow(dead_code, unused_imports)]

pub mod logs;

use std::sync::Arc;

use sc_audit::{AuditEvent, AuditEventKind, AuditWriter};
use sc_clock::ManualClock;
use sc_db::Db;
pub use sc_testdb::{START, TestDb};

pub struct Fixture {
    pub db: TestDb,
    pub api: Db,
    pub clock: Arc<ManualClock>,
    pub writer: AuditWriter,
}

impl Fixture {
    pub async fn new() -> Fixture {
        let db = TestDb::clone_template().await;
        let api = db.api().await;
        let clock = sc_testdb::clock_at_start();
        let writer = AuditWriter::new(api.clone(), clock.clone());
        Fixture {
            db,
            api,
            clock,
            writer,
        }
    }
}

pub fn event(kind: AuditEventKind) -> AuditEvent {
    AuditEvent::new(kind)
}
