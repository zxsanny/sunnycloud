//! Sharing performance suite PT-06-1..3 (AZ-3703) at the sharing port boundary on a real PostgreSQL 18 and the real
//! tree reader, over the link factory and the password corpus of the security suite. Each scenario has two tests:
//! a `functional` one that runs in CI on any host with a small load and asserts only what does not depend on the
//! hardware (0% errors, equal answers, permits and connections within their bounds), printing its times as
//! `NOT-RUN(HW-REF)`; and the full scenario with its timing thresholds, which are release gates valid on REF-SRV
//! (C-REF) only: it is ignored by default as `NOT-RUN(HW-REF)`, and a run on any other host is informational, never
//! a pass of the gate. On REF-SRV, in release, one case at a time:
//! `cargo test --release -p sunnycloud-sharing --test share_perf -- --ignored --nocapture --test-threads=1`.
//! Each case prints one `PT-06-n` line with its numbers and the host before it asserts.

#[allow(dead_code)]
#[path = "../share_security/corpora.rs"]
mod corpora;
#[allow(dead_code, unused_imports)]
#[path = "../sharing/harness.rs"]
mod harness;
#[allow(dead_code)]
#[path = "../share_security/link_factory.rs"]
mod link_factory;
mod load;
mod missing_uniformity;
mod open_mix;
#[allow(dead_code)]
#[path = "../sharing/owner_support.rs"]
mod owner_support;
mod password_flood;
#[allow(dead_code)]
#[path = "../sharing/settings.rs"]
mod settings;
#[allow(dead_code)]
#[path = "../sharing/visitor_support.rs"]
mod visitor_support;
