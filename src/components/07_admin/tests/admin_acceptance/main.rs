//! AZ-3823 (E-17/c09): ST-07-4 (`no_personal_data`) and AT-07-1 in its port form (`nothing_left`) over the real
//! components of the purger world, with the purger's v1.0 hook chain. Both read the JSON log lines of one
//! process-wide capturing subscriber. ST-07-1 and AT-07-2 are in `admin_security.rs`.

#[path = "../purger_logs/capture.rs"]
mod capture;
#[path = "../purge_support/mod.rs"]
mod purge_support;
#[path = "../support/mod.rs"]
mod support;

mod no_personal_data;
mod nothing_left;
mod ports;
