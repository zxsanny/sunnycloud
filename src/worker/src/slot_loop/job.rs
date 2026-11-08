use std::future::Future;

use media_shared::decoder_protocol::{Capabilities, Limits};
use media_shared::policy::{Plan, Probe, cpu_transcode_plan};
use media_shared::types::{ClaimedJob, DerivedKind, OutputMeta, SkipOutcome};

use super::config::KindConfig;
use super::failure::{Stop, decoder_stop, rerunnable};
use super::lease::Lease;
use super::metrics::JobMetrics;
use super::watchdog::{Tap, Watchdog};
use crate::api::{ApiError, WorkerApiClient};
use crate::decoder_client::{CallError, DecoderConn};
use crate::policy_step::{Step, StillRender, check_output, choose_step};
use crate::scratch::{ScratchDir, ScratchRoot};

/// What the decoder writes under `out/` and what the post names it. The API sniffs the bytes and decides.
const STILL_EXTENSION: &str = "webp";
const STILL_MEDIA_TYPE: &str = "image/webp";
const TRANSCODE_EXTENSION: &str = "mp4";
const TRANSCODE_MEDIA_TYPE: &str = "video/mp4";

/// How a job ended when every call it needed succeeded.
pub(super) enum Done {
    Derived,
    Remuxed,
    SkippedCompliant,
}

/// One claimed job on one gated connection: fetch, probe, the step the policy picks, the posts. The rerun
/// budget of the claim (one) is spent across all of its decoder calls.
pub(super) struct JobRun<'a> {
    pub(super) api: &'a WorkerApiClient,
    pub(super) job: &'a ClaimedJob,
    pub(super) root: &'a ScratchRoot,
    pub(super) scratch: &'a ScratchDir,
    pub(super) conn: &'a mut DecoderConn,
    pub(super) capabilities: &'a Capabilities,
    pub(super) kind: &'a KindConfig,
    pub(super) lease: &'a mut Lease,
    pub(super) watchdog: Watchdog,
    pub(super) metrics: &'a JobMetrics,
    pub(super) reran: bool,
}

/// A call to the API that is dropped the moment the lease is lost, so a long fetch or post does not outlive it.
async fn while_leased<T>(
    lease: &mut Lease,
    call: impl Future<Output = Result<T, ApiError>>,
) -> Result<T, Stop> {
    tokio::select! {
        biased;
        answer = call => answer.map_err(|error| Stop::from(&error)),
        end = lease.ended() => Err(Stop::from(end)),
    }
}

impl JobRun<'_> {
    pub(super) async fn run(&mut self) -> Result<Done, Stop> {
        self.root
            .ensure_fits(self.kind.max_input)
            .map_err(|error| Stop::from(&error))?;
        let source = self
            .scratch
            .create_source()
            .map_err(|error| Stop::from(&error))?;
        while_leased(self.lease, self.api.fetch_input(self.job, source)).await?;
        let job_dir = self
            .scratch
            .path()
            .to_str()
            .ok_or_else(Stop::scratch_io)?
            .to_owned();
        let probe = self.probe(&job_dir).await?;
        let step = choose_step(
            self.job.kind,
            self.job.params_version,
            &probe,
            self.capabilities,
        )?;
        match step {
            Step::Stills(renders) => {
                for render in renders {
                    self.still(&job_dir, render).await?;
                }
                Ok(Done::Derived)
            }
            Step::Poster(render) => {
                self.still(&job_dir, render).await?;
                Ok(Done::Derived)
            }
            Step::SkipCompliant => {
                self.api
                    .complete(self.job.job_id, SkipOutcome::SkippedCompliant)
                    .await
                    .map_err(|error| Stop::from(&error))?;
                Ok(Done::SkippedCompliant)
            }
            Step::Run(plan) => self.transcode(&job_dir, &probe, plan).await,
        }
    }

    fn limits(&self) -> Limits {
        self.kind.limits
    }

    /// Spends the claim's one rerun on `error`, or ends the job with it.
    fn grant_rerun(&mut self, error: &CallError) -> Result<(), Stop> {
        self.metrics.record_decoder_error(error);
        if rerunnable(error) && !self.reran {
            self.reran = true;
            Ok(())
        } else {
            Err(decoder_stop(error))
        }
    }

    async fn probe(&mut self, job_dir: &str) -> Result<Probe, Stop> {
        loop {
            let tap = Tap::new();
            let mut sink = tap.sink();
            let call = self.conn.probe(job_dir, &mut sink, tap.cancel());
            match self.watchdog.watch(self.lease, &tap, call).await? {
                Ok(probe) => return Ok(probe),
                Err(error) => self.grant_rerun(&error)?,
            }
        }
    }

    /// One render and its post. `out/` is emptied first, so a rerun and the second render of a `stills` job
    /// start clean.
    async fn still(&mut self, job_dir: &str, render: StillRender) -> Result<(), Stop> {
        let image = loop {
            self.scratch
                .clear_out()
                .map_err(|error| Stop::from(&error))?;
            let limits = self.limits();
            let tap = Tap::new();
            let mut sink = tap.sink();
            let call = self.conn.render_still(
                job_dir,
                render.max_edge,
                render.poster_at_ms,
                limits,
                &mut sink,
                tap.cancel(),
            );
            match self.watchdog.watch(self.lease, &tap, call).await? {
                Ok(image) => break image,
                Err(error) => self.grant_rerun(&error)?,
            }
        };
        let meta = OutputMeta {
            media_type: STILL_MEDIA_TYPE.to_owned(),
            width: Some(image.width),
            height: Some(image.height),
            duration_ms: None,
        };
        self.post(render.output, STILL_EXTENSION, &meta).await
    }

    async fn transcode(
        &mut self,
        job_dir: &str,
        input: &Probe,
        planned: Plan,
    ) -> Result<Done, Stop> {
        let mut plan = planned;
        let result = loop {
            self.scratch
                .clear_out()
                .map_err(|error| Stop::from(&error))?;
            let limits = self.limits();
            let tap = Tap::new();
            let mut sink = tap.sink();
            let call = self
                .conn
                .transcode(job_dir, plan, limits, &mut sink, tap.cancel());
            match self.watchdog.watch(self.lease, &tap, call).await? {
                Ok(result) => break result,
                Err(error) => {
                    self.grant_rerun(&error)?;
                    plan = cpu_transcode_plan(input);
                }
            }
        };
        check_output(&result.probe_of_output)?;
        let video = result.probe_of_output.video.as_ref();
        let meta = OutputMeta {
            media_type: TRANSCODE_MEDIA_TYPE.to_owned(),
            width: video.map(|stream| stream.width),
            height: video.map(|stream| stream.height),
            duration_ms: result.probe_of_output.duration_ms,
        };
        self.post(DerivedKind::Transcode, TRANSCODE_EXTENSION, &meta)
            .await?;
        Ok(match plan {
            Plan::Remux => Done::Remuxed,
            Plan::Transcode { .. } => Done::Derived,
        })
    }

    /// Safe-opens `out/result.<extension>` and posts it. "Already present" is success.
    async fn post(
        &mut self,
        kind: DerivedKind,
        extension: &str,
        meta: &OutputMeta,
    ) -> Result<(), Stop> {
        let output = self
            .scratch
            .open_output(extension, self.limits().max_output_bytes)
            .map_err(|error| Stop::from(&error))?;
        while_leased(
            self.lease,
            self.api.post_output(self.job.job_id, kind, meta, output),
        )
        .await
    }
}
