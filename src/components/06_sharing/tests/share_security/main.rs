//! Sharing security suite (AZ-3703) at the sharing port boundary on a real PostgreSQL 18 and the real tree
//! reader: ST-06-1 cross-account owner operations, ST-06-2 token strength and storage (with the chi-square
//! helper), ST-06-3 enumeration, ST-06-7 visitor disclosure and ST-06-8 row-level security with the one
//! cross-account lookup, over the link factory, the password corpus and the token corpus of 06 `tests.md`.
//! ST-06-4 and ST-06-5 run in the `sharing` binary (`visitor_grant.rs`, `visitor_password.rs`), ST-06-6 in
//! `share_abuse` (`password_limit.rs`); `cargo test -p sunnycloud-sharing` runs all eight. This binary installs the
//! log capture as its global subscriber (ST-06-2 reads the log lines), so it reuses the other binaries' fixtures by
//! path.

mod chi_square;
mod corpora;
mod cross_account;
mod disclosure;
mod enumeration;
#[allow(dead_code, unused_imports)]
#[path = "../sharing/harness.rs"]
mod harness;
mod link_factory;
#[allow(dead_code)]
#[path = "../share_abuse/logs.rs"]
mod logs;
mod lookup_scope;
#[allow(dead_code)]
#[path = "../sharing/owner_support.rs"]
mod owner_support;
#[allow(dead_code)]
#[path = "../sharing/settings.rs"]
mod settings;
mod token_strength;
#[allow(dead_code)]
#[path = "../sharing/visitor_support.rs"]
mod visitor_support;
