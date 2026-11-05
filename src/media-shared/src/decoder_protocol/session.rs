//! The session rules of one connection as a state check. The runner calls [`Session::admit`]
//! for each request it reads and [`Session::job_finished`] after a job's response is sent
//! (after the kill-all and the removal of `work/`).

use crate::decoder_protocol::{ProtocolViolation, Request, ViolationKind};

/// What the runner does with an admitted request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Serve it: answer `hello` and `self_check`, start a job, or cancel the running job.
    Accepted,
    /// A job request while a job runs: answer `DecodeError::Busy`; the running job continues.
    Busy,
}

#[derive(Debug, Default)]
pub struct Session {
    job_in_flight: bool,
}

fn is_job(request: &Request) -> bool {
    matches!(
        request,
        Request::Probe { .. } | Request::RenderStill { .. } | Request::Transcode { .. }
    )
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn job_in_flight(&self) -> bool {
        self.job_in_flight
    }

    /// `Err` is a hostile-class violation: `hello` or `self_check` while a job runs.
    pub fn admit(&mut self, request: &Request) -> Result<Admission, ProtocolViolation> {
        match (self.job_in_flight, request) {
            (false, request) if is_job(request) => {
                self.job_in_flight = true;
                Ok(Admission::Accepted)
            }
            (true, request) if is_job(request) => Ok(Admission::Busy),
            // A cancel on an idle session loses a race with the job's own response: no-op.
            (_, Request::Cancel) | (false, _) => Ok(Admission::Accepted),
            (true, _) => Err(ProtocolViolation(ViolationKind::OutOfSequence)),
        }
    }

    /// The job's response has been sent (or the job was cancelled).
    pub fn job_finished(&mut self) {
        self.job_in_flight = false;
    }
}
