//! Integration tests of the HTTP API crate over real TCP (Mode S harness). Later E-21 tasks add modules here.

mod abuse;
mod auth;
mod cap;
mod content;
mod csrf;
mod datapath;
mod errors;
mod h2_settings;
mod harness;
mod harness_scripting;
mod keepalive;
mod lifecycle;
mod limits;
mod middleware;
mod ops;
mod ops_metrics;
mod registry;
mod sharing;
mod shutdown;
mod version;
mod worker;
