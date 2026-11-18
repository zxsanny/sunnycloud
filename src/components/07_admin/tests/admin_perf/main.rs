//! Admin performance suite PT-07-1 and PT-07-2 (AZ-3824) on a real PostgreSQL 18 over the real components.
//! Each scenario has two tests: a `functional` one that runs in CI on any host with a small load and asserts only
//! what does not depend on the hardware (0% errors, correct answers, connections, statements per call, the end
//! state), printing its times as `NOT-RUN(HW-REF)`; and the full scenario with its timing thresholds, valid on
//! REF-SRV (C-REF) only: it is ignored by default as `NOT-RUN(HW-REF)`, and a run on any other host is
//! informational, never a pass. On REF-SRV, in release, one case at a time:
//! `cargo test --release -p sunnycloud-admin --test admin_perf -- --ignored --nocapture --test-threads=1`.
//! Each case prints one `PT-07-n` line with its numbers and the host with its hardware, then one line per metric
//! marked `PASS`, `TARGET-MISSED` or `FAIL` against its target and failure threshold, before it asserts.

#[path = "../purge_support/mod.rs"]
mod purge_support;
#[path = "../support/mod.rs"]
mod support;

mod background;
mod listing;
mod load;
mod pool_repo;
mod purge_load;
