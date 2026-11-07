//! Group-commit barrier (AZ-3727): real `fdatasync` on real files, with the `sync-hook` feature wrapping the
//! sync to block, delay, fail and count it. No database.

mod coalesce;
mod failure;
mod groups;
mod idle;
mod ordering;
mod real_sync;
mod release;
mod support;
