//! Orphan sweeper (AZ-3743): `OrphanSweeper` against a real PostgreSQL 18 and real files. Time is the `now` handed
//! to each cycle and the modification time set on each file; no test sleeps. Interleavings and database failures
//! are driven from the sweeper's test hook.

#[allow(dead_code)]
#[path = "../rows/support.rs"]
mod rows;

mod age;
mod modes;
mod names;
mod partial;
mod race;
mod setup;
mod support;
