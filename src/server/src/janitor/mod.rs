//! The janitor scheduler of `serve`: one timer loop per registered [`contracts::Janitor`], the janitor metrics
//! and the `scheduler_alive` readiness item, plus the audit retention janitor.
//!
//! The loops hold no logic: sleep until the interval, call `run_cycle(now)`, record the outcome. There is no
//! leader and no advisory lock; claims are the janitors' own (`SKIP LOCKED` or leases), and the scheduler
//! opens no database connection.

mod audit;
mod engine;
mod metrics;
mod readiness;

pub use audit::AuditRetentionJanitor;
pub use engine::{Scheduled, Scheduler, SchedulerError};
pub use metrics::JanitorMetrics;
pub use readiness::SchedulerAlive;
