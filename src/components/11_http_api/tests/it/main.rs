//! Integration tests of the HTTP API crate over real TCP (Mode S harness). Later E-21 tasks add modules here.

mod abuse;
mod cap;
mod errors;
mod h2_settings;
mod harness;
mod harness_scripting;
mod keepalive;
mod lifecycle;
mod ops;
mod ops_metrics;
mod registry;
