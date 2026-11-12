//! Security suite ST-04-1..8 (AZ-3747) at the port boundary: the real writer, reader, keyring, catalog, queue,
//! sweepers and scrubber on real files and a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL).
//!
//! ST-04-1 scans a plain blob root here; the scan of the whole loop-mounted XFS image (free space included) is
//! `NOT-RUN(C-priv)` unless the owner runs it on a mounted volume (see `stolen_disk.rs`).

#[allow(dead_code)]
#[path = "../scrub/support.rs"]
mod support;

mod accounts;
mod attacks;
mod hygiene;
mod keys;
mod kit;
mod rewrite;
mod stolen_disk;
mod traversal;
mod watch;
