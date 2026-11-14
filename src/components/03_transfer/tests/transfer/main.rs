//! Transfer skeleton (AZ-3733): settings, typed errors, the progress table and the `upload_sessions` and
//! `commit_receipts` migration; `start_upload` (AZ-3734); `put_block` (AZ-3735); `get_upload`, the lazy load and
//! `abort_upload` (AZ-3736); `stage_small` (AZ-3737); `commit_upload` (AZ-3738); `commit_batch` (AZ-3754); downloads (AZ-3755). All on a real PostgreSQL 18 (`DATABASE_URL`, a superuser URL).

mod abort;
mod access;
mod batch_commit;
mod batch_replay;
mod batch_support;
mod commit_ports;
mod commit_resume;
mod commit_support;
mod commit_target;
mod commit_upload;
mod config;
mod download_access;
mod download_failures;
mod download_ranges;
mod download_support;
mod errors;
mod migration;
mod progress;
mod put_caps;
mod put_checkpoint;
mod put_lanes;
mod put_support;
mod put_verify;
mod resume;
mod rig_config;
mod rls;
mod stage_refusals;
mod stage_small;
mod stage_support;
mod start_open;
mod start_ports;
mod start_refusals;
mod start_shortcut;
mod start_support;
mod support;
