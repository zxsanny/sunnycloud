//! IT-06-8 and ST-06-6 (AZ-3701, AC-20): the share abuse counters on a real `AbuseLimiter` driven by the harness's
//! manual clock, over a real PostgreSQL 18 and the real tree reader. Invalid tokens count per address on every
//! visitor route, wrong passwords per link whatever the address, a limited caller gets `RateLimited` with a retry
//! time, and each trip writes one audit event and one WARN line without the token. This binary installs the log
//! capture as its global subscriber, so it reuses the fixtures of the `sharing` binary by path.

#[allow(dead_code, unused_imports)]
#[path = "../sharing/harness.rs"]
mod harness;
#[allow(dead_code)]
mod logs;
#[allow(dead_code)]
#[path = "../sharing/owner_support.rs"]
mod owner_support;
mod password_limit;
mod rig;
#[allow(dead_code)]
#[path = "../sharing/settings.rs"]
mod settings;
mod token_limit;
mod trip_audit;
#[allow(dead_code)]
#[path = "../sharing/visitor_support.rs"]
mod visitor_support;
