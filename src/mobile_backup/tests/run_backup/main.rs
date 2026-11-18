//! The `run_backup` loop (AZ-3990) against a fake source and a fake sync API, over the real SQLite checkpoint.

mod ack_before_uploaded;
mod edits_and_pending;
mod folder;
mod hash_ahead;
mod new_assets;
mod one_run;
mod routing;
mod support;
mod two_processes;
mod visible_diff;
