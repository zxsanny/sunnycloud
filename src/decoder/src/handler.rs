//! The handler of the real runner: the executors behind the socket. Each job gets its own
//! `work/`, removed on every way out of the job.

use std::path::Path;

use media_shared::decoder_protocol::{Capabilities, DecodeError, Response, SelfCheckReport};

use sc_observability::log_error;

use crate::config::RunnerConfig;
use crate::containment::{Sweep, kill_all_descendants};
use crate::exec::{Exec, ExecutorConfig, Work, internal};
use crate::failure::Failure;
use crate::image;
use crate::job::{Job, JobContext, JobHandler, JobSpec};
use crate::params;
use crate::tools::Tools;
use crate::video;

pub struct MediaHandler {
    tools: Tools,
    config: ExecutorConfig,
}

impl MediaHandler {
    pub fn new(tools: Tools, config: &RunnerConfig) -> Self {
        Self {
            tools,
            config: ExecutorConfig {
                photo_max_pixels: config.photo_max_pixels,
                memory_poll: config.memory_poll_interval,
            },
        }
    }

    fn execute(
        &self,
        job: &Job,
        context: &JobContext,
        job_dir: &Path,
        work: &Work,
    ) -> Result<Response, Failure> {
        let exec = Exec {
            tools: &self.tools,
            config: &self.config,
            limits: &job.limits,
            context,
            job_dir,
            work,
        };
        match &job.spec {
            JobSpec::Probe { .. } => image::probe(&exec),
            JobSpec::RenderStill {
                max_edge,
                poster_at_ms,
                params_version,
                ..
            } => {
                let params = params::for_version(*params_version)?;
                image::render_still(&exec, params, *max_edge, poster_at_ms.unwrap_or(0))
            }
            JobSpec::Transcode { plan, .. } => video::transcode(&exec, *plan),
        }
    }
}

impl JobHandler for MediaHandler {
    fn capabilities(&self) -> Capabilities {
        // The tool versions and the build timestamp are E-19/c12; "unknown" and 1 (the lowest
        // timestamp the protocol accepts) mean "not reported".
        Capabilities {
            protocol: crate::PROTOCOL_VERSION,
            vips: "unknown".into(),
            ffmpeg: "unknown".into(),
            libheif: "unknown".into(),
            build_timestamp: 1,
            nvenc_probe_ok: false,
            nvenc_sessions: 0,
        }
    }

    fn self_check(&self, _forbidden_targets: &[String]) -> SelfCheckReport {
        crate::self_check::report()
    }

    /// Runs the job, then kills and reaps every descendant of the runner and removes `work/`,
    /// and only then returns the response (10 section 3, containment). The order holds on every
    /// way out of the job: success, a typed failure, a guard kill and a cancel.
    fn run(&self, job: &Job, context: &JobContext) -> Response {
        let job_dir = Path::new(job.spec.job_dir());
        let work = Work::create(job_dir).map_err(|error| internal("cannot make work/", &error));
        let result = match &work {
            Ok(work) => self.execute(job, context, job_dir, work),
            Err(failure) => Err(*failure),
        };
        let sweep = kill_all_descendants();
        drop(work);
        let mut hostile = result
            .as_ref()
            .err()
            .is_some_and(|failure| failure.is_hostile());
        let mut response = result.unwrap_or_else(Failure::response);
        match sweep {
            Ok(Sweep::Clean) => {}
            Ok(Sweep::KilledLive) => {
                log_error!(
                    "a descendant of the runner was alive at the end of a job and was killed"
                );
                hostile = true;
                response = Response::Error(DecodeError::OutputInvalid);
            }
            Err(error) => {
                log_error!(
                    "descendants of the runner could not be killed",
                    err = &error
                );
                hostile = true;
                response = Response::Error(DecodeError::OutputInvalid);
            }
        }
        if hostile {
            context.mark_hostile();
        }
        response
    }
}
