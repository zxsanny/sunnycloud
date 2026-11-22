//! The seam between the socket loop and the executors (E-19/c08 to c11): what a job is, what
//! the loop gives the handler, and what the handler may tell the loop while it runs.

use std::fs::File;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use media_shared::decoder_protocol::{Capabilities, Response, SelfCheckReport};
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
        params_version: u32,
    },
    Transcode {
        job_dir: String,
        plan: Plan,
    },
}

impl JobSpec {
    pub fn job_dir(&self) -> &str {
        match self {
            JobSpec::Probe { job_dir }
            | JobSpec::RenderStill { job_dir, .. }
            | JobSpec::Transcode { job_dir, .. } => job_dir,
        }
    }
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

/// What a running job sees of the loop: the verified input, the cancel flag and the progress
/// channel.
pub struct JobContext {
    input: File,
    cancel: Arc<AtomicBool>,
    messages: Sender<JobMessage>,
    hostile: Arc<AtomicBool>,
}

impl JobContext {
    pub(crate) fn new(
        input: File,
        cancel: Arc<AtomicBool>,
        messages: Sender<JobMessage>,
        hostile: Arc<AtomicBool>,
    ) -> Self {
        Self {
            input,
            cancel,
            messages,
            hostile,
        }
    }

    /// `in/source`, already opened by the safe open: a regular file with one link, in the
    /// job's own directory. An executor reads this file and never opens `job_dir/in/source`
    /// by path itself.
    pub fn input(&self) -> &File {
        &self.input
    }

    /// True once the client sent `cancel`, the connection closed, or the runner is ending.
    /// A job that sees it kills its processes, removes `work/` and returns.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    /// Marks the job's outcome as hostile-class (10 section 3, containment): the loop sends the
    /// typed response and then ends the runner so that the container is restarted clean.
    pub fn mark_hostile(&self) {
        self.hostile.store(true, Ordering::Release);
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
    /// The reply to a `hello` with protocol 2. The loop sets `protocol` itself.
    fn capabilities(&self) -> Capabilities;

    fn self_check(&self, forbidden_targets: &[String]) -> SelfCheckReport;

    /// Runs the job to its response. By the time it returns, its processes are killed and its
    /// `work/` is removed (10 section 3). Must poll [`JobContext::is_cancelled`].
    fn run(&self, job: &Job, context: &JobContext) -> Response;
}
