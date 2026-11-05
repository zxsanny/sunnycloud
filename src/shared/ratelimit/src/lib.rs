//! In-memory abuse limiter for the AC-20 failures (server only).
//!
//! One token bucket per `(LimitId, Subject)`, driven by the injected monotonic clock. There is no
//! generic per-route limiter in v1.0. Counters are per process and reset on restart; v1.0 runs one API
//! process, so they are exact (a second replica would multiply every limit).
//!
//! # Limit table
//!
//! | `LimitId` | `limit` label | Subject | Default | Config key for a non-default number |
//! |---|---|---|---|---|
//! | `SigninIp` | `signin_ip` | IP | 10 per minute | none (compiled AC-20 number) |
//! | `DeviceGrantIp` | `device_grant_ip` | IP | no AC-20 number: caller supplies it | `device_grant_ip_limit` (count only, no window key) |
//! | `DeviceApprovalSession` | `device_approval_session` | session | 5 per hour | none (compiled) |
//! | `DeviceApprovalAccount` | `device_approval_account` | account | 20 per hour | none (compiled) |
//! | `DeviceApprovalIp` | `device_approval_ip` | IP | 50 per hour | none (compiled) |
//! | `InvalidTokenIp` | `invalid_token_ip` | IP | no AC-20 number: caller supplies it | `invalid_token_ip_limit` (count only, no window key) |
//! | `SharePasswordLink` | `share_password` | link | 10 per 10 minutes | `share_wrong_password_limit`, `share_wrong_password_window` |
//! | `ShareTokenIp` | `share_token_ip` | IP | 100 per minute | `share_invalid_token_limit` (count only, window is one minute) |
//!
//! The bucket cap is `ratelimit_max_entries` (default [`DEFAULT_MAX_ENTRIES`]); [`LimitTable`] carries it.
//!
//! # Behaviour
//!
//! * A bucket holds `capacity` tokens and refills at `capacity / window`. Each [`AbuseLimiter::record_failure`]
//!   takes one. The call that takes the last token returns [`Verdict::Tripped`] (the caller audits once);
//!   later calls on the empty bucket return [`Verdict::Limited`] and take nothing. After a partial refill
//!   the next failure can exhaust the bucket again, which is a new exhaustion and trips again.
//! * [`AbuseLimiter::check`] reads only. Callers check before costly work and record after a failure.
//! * Memory: the map is sharded behind mutexes (no lock is held across an `await`; the API is
//!   synchronous). A new bucket first drops the oldest buckets that have refilled to capacity, then
//!   evicts the oldest idle ones until it fits, so the map never exceeds `ratelimit_max_entries`.
//!   Hash lookups are O(1); the idle ordering inside a shard is a `BTreeMap`, so insertion is
//!   O(log shard size).

mod bucket;
mod limiter;
mod stats;
mod table;

pub use limiter::AbuseLimiter;
pub use stats::{LimitCounts, LimiterStats};
pub use table::{
    ConfigError, DEFAULT_MAX_ENTRIES, LimitId, LimitSpec, LimitTable, Subject, Verdict,
};
