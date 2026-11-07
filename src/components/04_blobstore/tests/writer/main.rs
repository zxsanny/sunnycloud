//! `BlobWriter` (AZ-3726): real encryption, real `pwrite`, `fallocate` and `statvfs` on real files, and the real
//! `fdatasync` through the group-commit barrier (the `sync-hook` feature only delays, fails and counts it).
//! A real PostgreSQL holds the key check of the keyring and proves that the writer creates no `blobs` row.
//!
//! Rows that need a loop volume and a privileged runner (a real full volume, a 64 MiB volume with an 8 MiB
//! reserve) are `NOT-RUN(C-priv)`; the floor and the no-space refusals are exercised here against the real
//! volume of the scratch folder with a reserve or a declared size beyond what it can hold.

mod complete;
mod create;
mod durable;
mod layout;
mod plan;
mod reopen;
mod support;
