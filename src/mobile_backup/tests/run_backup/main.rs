//! The `run_backup` loop (AZ-3990, AZ-3991) against a fake source and a fake sync API, over the real SQLite checkpoint.

mod ack_before_uploaded;
mod camera_folder;
mod cancel;
mod counts;
mod edits_and_pending;
mod folder;
#[cfg(feature = "uniffi")]
mod foreign_source;
mod hash_ahead;
mod new_assets;
mod no_token;
mod one_run;
mod routing;
mod status;
mod support;
mod two_processes;
mod upload_names;
mod visible_diff;
mod wire_errors;
