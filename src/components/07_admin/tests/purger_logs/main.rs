//! AZ-3820 AC-2 (IT-07-10) and AC-6: what the purger logs. One process-wide subscriber captures every JSON line
//! (a per-thread default races with the callsite interest cache when tests run in parallel); each test reads the
//! lines that carry its own account id.

#[path = "../purge_support/mod.rs"]
mod purge_support;
#[path = "../support/mod.rs"]
mod support;

mod alert;
mod capture;
mod resume;
