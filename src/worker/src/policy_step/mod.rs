//! The pure step chooser of the slot loop (09 section 3, step 3; E-20/c10a): what to render or run for a
//! claimed job, from its probe and the slot's cached decoder capabilities, and the check of a transcode
//! output. Every rule comes from `media_shared::policy`; nothing here restates one. No I/O, no clock.

use media_shared::decoder_protocol::Capabilities;
use media_shared::policy::{
    Decision, EncoderPref, Params, Plan, Probe, decide, is_compliant, poster_offset_ms,
};
use media_shared::types::{DerivedKind, Disposition, FailureReport, JobKind};
use thiserror::Error;

/// One `render_still` call and the output it is posted as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StillRender {
    pub max_edge: u32,
    pub poster_at_ms: Option<u64>,
    pub output: DerivedKind,
}

/// What the slot loop executes for one claimed job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Two renders from the same input, in order: preview, then thumb.
    Stills([StillRender; 2]),
    Poster(StillRender),
    /// The clip is compliant: close the job `skipped_compliant` with no output.
    SkipCompliant,
    /// Call `transcode` with this plan (`Remux` or a `Transcode`) and post `outputs/transcode`.
    Run(Plan),
}

/// Why no step can be chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum StepError {
    #[error("params_version {0} is not known to this build")]
    UnknownParamsVersion(u32),
    #[error("the probe of a poster job reports no duration")]
    MissingDuration,
}

/// The GPU encoder only when the operator enabled it and the decoder proved it by a test encode (AC-28).
fn encoder_pref(nvenc_enabled: bool, caps: &Capabilities) -> EncoderPref {
    if nvenc_enabled && caps.nvenc_probe_ok {
        EncoderPref::Nvenc
    } else {
        EncoderPref::Cpu
    }
}

/// Chooses the step of a claimed job. `caps` are the capabilities the slot cached at `hello`; `nvenc_enabled`
/// is the operator's `nvenc = on`.
pub fn choose_step(
    kind: JobKind,
    params_version: u32,
    probe: &Probe,
    caps: &Capabilities,
    nvenc_enabled: bool,
) -> Result<Step, StepError> {
    let params = Params::for_version(params_version)
        .ok_or(StepError::UnknownParamsVersion(params_version))?;
    match kind {
        JobKind::Stills => Ok(Step::Stills([
            still(params.preview_edge, None, DerivedKind::Preview),
            still(params.thumb_edge, None, DerivedKind::Thumb),
        ])),
        JobKind::Poster => {
            let duration = probe.duration_ms.ok_or(StepError::MissingDuration)?;
            Ok(Step::Poster(still(
                params.thumb_edge,
                Some(poster_offset_ms(duration)),
                DerivedKind::Thumb,
            )))
        }
        JobKind::Transcode => Ok(
            match decide(probe, caps, encoder_pref(nvenc_enabled, caps)) {
                Decision::SkipCompliant => Step::SkipCompliant,
                Decision::Remux => Step::Run(Plan::Remux),
                Decision::Transcode(plan) => Step::Run(plan),
            },
        ),
    }
}

fn still(max_edge: u32, poster_at_ms: Option<u64>, output: DerivedKind) -> StillRender {
    StillRender {
        max_edge,
        poster_at_ms,
        output,
    }
}

/// The decoder produced a `Remux` or `Transcode` output that the shared policy does not accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("the decoder's transcode output is not compliant")]
pub struct OutputInvalid;

impl OutputInvalid {
    /// The failure code the API receives (`worker_supervisor` tests, IT-09-5).
    pub const CODE: &'static str = "OutputInvalid";

    /// `Permanent`: the same input would produce the same output again.
    pub fn failure_report(self) -> FailureReport {
        FailureReport {
            code: Self::CODE.to_owned(),
            disposition: Disposition::Permanent,
        }
    }

    /// `OutputInvalid` is not `Stalled` or `EncoderFailed`: no rerun on the CPU.
    pub const fn cpu_rerun(self) -> bool {
        false
    }
}

/// Applies the shared compliance rule to `probe_of_output` of a transcode reply.
pub fn check_output(probe_of_output: &Probe) -> Result<(), OutputInvalid> {
    if is_compliant(probe_of_output) {
        Ok(())
    } else {
        Err(OutputInvalid)
    }
}

#[cfg(test)]
mod tests;
