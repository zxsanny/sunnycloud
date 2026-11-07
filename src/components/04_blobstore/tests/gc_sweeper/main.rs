//! GC sweeper (AZ-3730): `GcSweeper` against a real PostgreSQL 18 and real files, with a recording
//! `BlobOwnerCleanup`. Time is the manual clock and the `now` handed to each cycle; no test sleeps or measures
//! wall time. Interleavings are driven by barriers in the sweeper's test hook.

#[allow(dead_code)]
#[path = "../rows/support.rs"]
mod rows;

mod backlog;
mod crash;
mod lease;
mod race;
mod support;
