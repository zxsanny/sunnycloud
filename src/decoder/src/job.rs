//! The seam between the socket loop and the executors (E-19/c08 to c11): what a job is, what
//! the loop gives the handler, and what the handler may tell the loop while it runs.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use media_shared::decoder_protocol::{
    Capabilities, DecodeError, Response, SelfCheckItem, SelfCheckReport,
};
use media_shared::policy::Plan;

use crate::limits::{JobLimits, LimitClass};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobSpec {
    Probe {
        job_dir: String,
    },
    RenderStill {
        job_dir: String,
        max_edge: u32,
        poster_at_ms: Option<u64>,
    },
    Transcode {
        job_dir: String,
        plan: Plan,
    },
}

/// One admitted job request with the limits it runs under, already clamped to the ceilings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub spec: JobSpec,
    pub class: LimitClass,
    pub limits: JobLimits,
}

pub(crate) enum JobMessage {
    Progress {
        out_time_ms: u64,
        speed: f64,
    },
    /// Boxed: a `Response` is far larger than a progress line.
    Done(Box<Response>),
}

/// What a running job sees of the loop: the cancel flag and the progress channel.
pub struct JobContext {
    cancel: Arc<AtomicBool>,
    messages: Sender<JobMessage>,
}

impl JobContext {
    pub(crate) fn new(cancel: Arc<AtomicBool>, messages: Sender<JobMessage>) -> Self {
        Self { cancel, messages }
    }

    /// True once the client sent `cancel`, the connection closed, or the runner is ending.
    /// A job that sees it kills its processes, removes `work/` and returns.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    /// Forwards one executor progress line as a `Progress` event.
    pub fn progress(&self, out_time_ms: u64, speed: f64) {
        let message = JobMessage::Progress { out_time_ms, speed };
        if self.messages.send(message).is_err() {
            // The loop dropped its receiver, so the connection is gone: the job must stop.
            self.cancel.store(true, Ordering::Release);
        }
    }
}

/// The executors behind the socket. Called from the connection thread (`capabilities`,
/// `self_check`) and from one job thread at a time (`run`).
pub trait JobHandler: Send + Sync + 'static {
    /// The reply to a `hello` with protocol 1. The loop sets `protocol` itself.
    fn capabilities(&self) -> Capabilities;

    fn self_check(&self, forbidden_targets: &[String]) -> SelfCheckReport;

    /// Runs the job to its response. By the time it returns, its processes are killed and its
    /// `work/` is removed (10 section 3). Must poll [`JobContext::is_cancelled`].
    fn run(&self, job: &Job, context: &JobContext) -> Response;
}

/// The handler of this build: no executor exists yet (E-19/c08 to c11), so no job is decoded
/// and every job request is answered `Unsupported`. It never produces a result.
pub struct NoExecutors;

impl JobHandler for NoExecutors {
    fn capabilities(&self) -> Capabilities {
        // The versions and the build timestamp are E-19/c12; 1 is the lowest timestamp the
        // protocol accepts and marks "unknown".
        Capabilities {
            protocol: crate::PROTOCOL_VERSION,
            vips: "unavailable".into(),
            ffmpeg: "unavailable".into(),
            libheif: "unavailable".into(),
            build_timestamp: 1,
            nvenc_probe_ok: false,
            nvenc_sessions: 0,
        }
    }

    fn self_check(&self, _forbidden_targets: &[String]) -> SelfCheckReport {
        SelfCheckReport(vec![SelfCheckItem {
            name: "executors".into(),
            ok: false,
        }])
    }

    fn run(&self, _job: &Job, _context: &JobContext) -> Response {
        sc_observability::log_error!("job refused: this build has no decode executors");
        Response::Error(DecodeError::Unsupported)
    }
}
