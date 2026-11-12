//! The `decoder-runner` process (component 10): the Unix socket server of decoder protocol v1.
//!
//! One connection and one job at a time. The framing, the strict parser and the session rules
//! come from `media_shared::decoder_protocol`; this crate adds the runner side:
//!
//! - `hello`, `self_check` and `cancel` are answered by the loop; `probe`, `render_still` and
//!   `transcode` go to a [`JobHandler`] on a job thread (the executors, E-19/c08 to c11).
//! - `Alive` goes out every `decoder_alive_interval` while a job runs and never while idle;
//!   each [`JobContext::progress`] call is one `Progress` event.
//! - `cancel` sets the job's cancel flag; the job's own response is replaced by `Cancelled`.
//!   Connection loss sets the same flag and sends nothing.
//! - A `cancel` on an idle session is a no-op and gets no reply (the session rule: it lost a
//!   race with the job's response).
//! - Every requested limit is clamped to the runner's ceilings ([`JobLimits::clamp`]) before
//!   the handler sees it. A `probe` carries no limits and runs under the photo ceilings.
//! - Before the handler sees a job, `job_dir` is resolved inside `decoder_scratch_root` with no
//!   symlink and `in/source` is opened as a regular single-link file ([`safe_open`]); anything
//!   else is answered `Corrupt` and no handler runs.
//! - The loop bumps a [`Heartbeat`] on every pass and a [`Watchdog`] thread aborts the process
//!   when it stops moving for `decoder_watchdog`. With the `test-freeze` feature (test images
//!   only) `SIGUSR1` freezes the loop so that this can be shown.
//! - A client violation is answered `ProtocolViolation` and [`serve`] returns
//!   [`Exit::ProtocolViolation`]; the binary then exits with code 2.

pub mod config;
mod connection;
pub mod error;
pub mod health;
pub mod job;
pub mod limits;
pub mod safe_open;
pub mod server;
pub mod socket;
mod sys;
#[cfg(feature = "test-freeze")]
pub mod test_freeze;
pub mod watchdog;

pub use config::RunnerConfig;
pub use connection::TICK;
pub use error::RunnerError;
pub use job::{Job, JobContext, JobHandler, JobSpec, NoExecutors};
pub use limits::{Ceilings, JobLimits, LimitClass};
pub use server::{Exit, serve};
pub use watchdog::{Heartbeat, Watchdog, disable_dumps};

/// The only protocol version this runner speaks.
pub const PROTOCOL_VERSION: u32 = 1;
