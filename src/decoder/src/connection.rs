//! One connection: reads requests, applies the session rules, runs at most one job on a job
//! thread and writes the replies and events. The loop is single threaded and wakes at least
//! every [`TICK`], so waiting for a frame never blocks the alive timer or a heartbeat.

use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{
    Admission, DecodeError, Event, FrameReader, Message, Polled, ProtocolViolation, ReadError,
    Request, Response, Session, WriteError, write_message,
};
use sc_observability::{log_error, log_info, log_warn};

use crate::config::RunnerConfig;
use crate::error::RunnerError;
use crate::job::{Job, JobContext, JobHandler, JobMessage, JobSpec};
use crate::limits::LimitClass;
use crate::safe_open::open_input;
use crate::watchdog::Heartbeat;

/// Longest wait of any blocking call of the loop.
pub const TICK: Duration = Duration::from_millis(50);

const JOB_STOP_POLL: Duration = Duration::from_millis(5);

#[derive(Debug)]
pub enum ConnectionEnd {
    /// The peer closed the connection or it failed; the runner accepts the next one.
    Closed,
    /// The peer broke the protocol; the typed reply was sent. The runner exits.
    Violation,
    /// The runner cannot continue and exits.
    Fatal(RunnerError),
}

struct RunningJob {
    cancel: Arc<AtomicBool>,
    messages: Receiver<JobMessage>,
    thread: JoinHandle<()>,
    cancel_requested: bool,
    started: Instant,
}

enum Dispatch {
    Answer(Response),
    Cancel,
    Start(Job),
}

struct Connection<'a, H> {
    stream: UnixStream,
    handler: &'a Arc<H>,
    config: &'a RunnerConfig,
    heartbeat: &'a Heartbeat,
    reader: FrameReader,
    session: Session,
    job: Option<RunningJob>,
    next_alive: Instant,
}

pub fn serve_connection<H: JobHandler>(
    stream: UnixStream,
    handler: &Arc<H>,
    config: &RunnerConfig,
    heartbeat: &Heartbeat,
) -> ConnectionEnd {
    let timeouts = stream
        .set_read_timeout(Some(TICK))
        .and_then(|()| stream.set_write_timeout(Some(config.write_timeout)));
    if let Err(source) = timeouts {
        return ConnectionEnd::Fatal(RunnerError::Io {
            what: "set socket timeouts",
            source,
        });
    }
    let mut connection = Connection {
        stream,
        handler,
        config,
        heartbeat,
        reader: FrameReader::new(),
        session: Session::new(),
        job: None,
        next_alive: Instant::now(),
    };
    let end = loop {
        if let Err(end) = connection.step() {
            break end;
        }
    };
    connection.finish(end)
}

impl<H: JobHandler> Connection<'_, H> {
    fn step(&mut self) -> Result<(), ConnectionEnd> {
        self.heartbeat.beat();
        self.forward_job_messages()?;
        self.send_alive_if_due()?;
        match self.reader.poll_request(&mut self.stream) {
            Ok(Polled::Ready(request)) => self.handle(request),
            Ok(Polled::Pending) => Ok(()),
            Ok(Polled::Closed) => Err(ConnectionEnd::Closed),
            Err(ReadError::Violation(violation)) => Err(self.violation(violation)),
            Err(ReadError::Io(error)) => {
                log_warn!("decoder socket read failed", err = &error);
                Err(ConnectionEnd::Closed)
            }
        }
    }

    fn handle(&mut self, request: Request) -> Result<(), ConnectionEnd> {
        match self.session.admit(&request) {
            Err(violation) => Err(self.violation(violation)),
            Ok(Admission::Busy) => self.reply(Response::Error(DecodeError::Busy)),
            Ok(Admission::Accepted) => match self.dispatch(request) {
                Dispatch::Answer(response) => self.reply(response),
                Dispatch::Cancel => {
                    if let Some(job) = &mut self.job {
                        job.cancel.store(true, Ordering::Release);
                        job.cancel_requested = true;
                    }
                    Ok(())
                }
                Dispatch::Start(job) => self.start(job),
            },
        }
    }

    fn dispatch(&self, request: Request) -> Dispatch {
        let ceilings = &self.config.ceilings;
        match request {
            Request::Hello { protocol } if protocol == crate::PROTOCOL_VERSION => {
                let mut capabilities = self.handler.capabilities();
                capabilities.protocol = crate::PROTOCOL_VERSION;
                Dispatch::Answer(Response::Capabilities(capabilities))
            }
            Request::Hello { .. } => {
                Dispatch::Answer(Response::Error(DecodeError::ProtocolMismatch))
            }
            Request::SelfCheck { forbidden_targets } => Dispatch::Answer(Response::SelfCheck(
                self.handler.self_check(&forbidden_targets),
            )),
            Request::Cancel => Dispatch::Cancel,
            // A probe has no limits of its own and runs under the lower, photo, ceilings.
            Request::Probe { job_dir } => Dispatch::Start(Job {
                spec: JobSpec::Probe { job_dir },
                class: LimitClass::Photo,
                limits: ceilings.photo,
            }),
            Request::RenderStill {
                job_dir,
                max_edge,
                poster_at_ms,
                limits,
            } => Dispatch::Start(Job {
                spec: JobSpec::RenderStill {
                    job_dir,
                    max_edge,
                    poster_at_ms,
                },
                class: LimitClass::Photo,
                limits: ceilings.photo.clamp(&limits),
            }),
            Request::Transcode {
                job_dir,
                plan,
                limits,
            } => Dispatch::Start(Job {
                spec: JobSpec::Transcode { job_dir, plan },
                class: LimitClass::Video,
                limits: ceilings.video.clamp(&limits),
            }),
        }
    }

    fn start(&mut self, job: Job) -> Result<(), ConnectionEnd> {
        let input = match open_input(&self.config.scratch_root, job.spec.job_dir()) {
            Ok(input) => input,
            Err(refused) => {
                log_warn!("job input refused", err = &refused);
                self.session.job_finished();
                return self.reply(Response::Error(DecodeError::Corrupt));
            }
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, messages) = channel();
        let context = JobContext::new(input, Arc::clone(&cancel), sender.clone());
        let handler = Arc::clone(self.handler);
        let spawned = thread::Builder::new()
            .name("decoder-job".into())
            .spawn(move || {
                let response = handler.run(&job, &context);
                // The receiver is gone only when the connection already ended.
                if sender.send(JobMessage::Done(Box::new(response))).is_err() {
                    log_warn!("job finished after its connection ended");
                }
            });
        match spawned {
            Ok(thread) => {
                self.job = Some(RunningJob {
                    cancel,
                    messages,
                    thread,
                    cancel_requested: false,
                    started: Instant::now(),
                });
                self.next_alive = Instant::now() + self.config.alive_interval;
                Ok(())
            }
            Err(source) => Err(ConnectionEnd::Fatal(RunnerError::Io {
                what: "spawn job thread",
                source,
            })),
        }
    }

    fn forward_job_messages(&mut self) -> Result<(), ConnectionEnd> {
        loop {
            let Some(job) = &self.job else {
                return Ok(());
            };
            match job.messages.try_recv() {
                Ok(JobMessage::Progress { out_time_ms, speed }) => {
                    self.send_progress(Event::Progress { out_time_ms, speed })?;
                }
                Ok(JobMessage::Done(response)) => return self.complete(*response),
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    log_error!("job thread ended without a response");
                    return Err(ConnectionEnd::Fatal(RunnerError::JobPanicked));
                }
            }
        }
    }

    /// The job returned: its processes are gone and `work/` is removed, so the response goes out.
    fn complete(&mut self, response: Response) -> Result<(), ConnectionEnd> {
        let Some(job) = self.job.take() else {
            return Ok(());
        };
        if job.thread.join().is_err() {
            return Err(ConnectionEnd::Fatal(RunnerError::JobPanicked));
        }
        self.session.job_finished();
        let duration_ms = u64::try_from(job.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        log_info!("job finished", duration_ms = duration_ms);
        let response = if job.cancel_requested {
            Response::Cancelled
        } else {
            response
        };
        match self.send(Message::Response(response)) {
            Err(WriteError::Invalid(violation)) => {
                log_error!("job response refused by the protocol", err = &violation);
                self.reply(Response::Error(DecodeError::OutputInvalid))
            }
            other => other.map_err(|error| self.write_failed(error)),
        }
    }

    fn send_alive_if_due(&mut self) -> Result<(), ConnectionEnd> {
        let interval = self.config.alive_interval;
        let now = Instant::now();
        if self.job.is_none() || now < self.next_alive {
            return Ok(());
        }
        self.next_alive += interval;
        if self.next_alive <= now {
            self.next_alive = now + interval;
        }
        self.send(Message::Event(Event::Alive))
            .map_err(|error| self.write_failed(error))
    }

    fn send_progress(&mut self, event: Event) -> Result<(), ConnectionEnd> {
        match self.send(Message::Event(event)) {
            Err(WriteError::Invalid(violation)) => {
                log_error!("progress event refused by the protocol", err = &violation);
                Ok(())
            }
            other => other.map_err(|error| self.write_failed(error)),
        }
    }

    fn reply(&mut self, response: Response) -> Result<(), ConnectionEnd> {
        self.send(Message::Response(response))
            .map_err(|error| self.write_failed(error))
    }

    fn send(&mut self, message: Message) -> Result<(), WriteError> {
        write_message(&mut self.stream, &message)
    }

    fn write_failed(&self, error: WriteError) -> ConnectionEnd {
        match error {
            WriteError::Io(error) => {
                log_warn!("decoder socket write failed", err = &error);
                ConnectionEnd::Closed
            }
            WriteError::Invalid(violation) => {
                log_error!("reply refused by the protocol", err = &violation);
                ConnectionEnd::Fatal(RunnerError::Violation(violation))
            }
        }
    }

    /// Replies `ProtocolViolation` (best effort: the peer is hostile) and ends the runner.
    fn violation(&mut self, violation: ProtocolViolation) -> ConnectionEnd {
        log_error!("client protocol violation", err = &violation);
        let reply = Message::Response(Response::Error(DecodeError::ProtocolViolation));
        if let Err(error) = self.send(reply) {
            log_warn!("violation reply not delivered", err = &error);
        }
        ConnectionEnd::Violation
    }

    /// Ends the connection. A running job gets its cancel flag; connection loss and violation
    /// send no further reply. A job that does not return within the cancel wait ends the runner:
    /// a second job must never run beside it.
    fn finish(mut self, end: ConnectionEnd) -> ConnectionEnd {
        let Some(job) = self.job.take() else {
            return end;
        };
        job.cancel.store(true, Ordering::Release);
        if !matches!(end, ConnectionEnd::Fatal(_)) {
            log_warn!("connection ended while a job ran: job cancelled");
        }
        if let Err(error) = wait_for_stop(&job.thread, self.config.cancel_wait, self.heartbeat) {
            log_error!("job did not stop after cancel", err = &error);
            return ConnectionEnd::Fatal(error);
        }
        match job.thread.join() {
            Ok(()) => end,
            Err(_) => ConnectionEnd::Fatal(RunnerError::JobPanicked),
        }
    }
}

fn wait_for_stop(
    thread: &JoinHandle<()>,
    limit: Duration,
    heartbeat: &Heartbeat,
) -> Result<(), RunnerError> {
    let deadline = Instant::now() + limit;
    while !thread.is_finished() {
        heartbeat.beat();
        if Instant::now() >= deadline {
            return Err(RunnerError::JobStuck);
        }
        thread::sleep(JOB_STOP_POLL);
    }
    Ok(())
}
