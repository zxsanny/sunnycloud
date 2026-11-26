//! AZ-3813: the cross-command cases of IT-12 at the CLI boundary. The per-command cases live in the command tasks'
//! test files; this binary owns what no single command task proves: one exit and JSON rule for every command, the
//! single-business-call check over the contract's command table, the secret channel across commands, and the
//! coverage map of IT-12-1..19 (`it12_coverage_map.md`, kept in `tests/`).

mod common;
#[path = "../config_support/mod.rs"]
mod config_support;
mod coverage_map;
mod exit_rules;
mod lazy;
mod one_operation;
mod secret_channel;
#[path = "../support/mod.rs"]
mod support;
