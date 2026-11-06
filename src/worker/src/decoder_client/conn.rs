use std::future::{self, Future};
use std::io::{self, ErrorKind, Read};
use std::path::Path;

use media_shared::decoder_protocol::{
    Capabilities, DecodeError, Event, FrameReader, ImageResult, Limits, Message, Polled,
    ProtocolViolation, ReadError, Reply, Request, Response, SelfCheckReport, TranscodeResult,
    ViolationKind, WriteError, write_message,
};
use media_shared::policy::{Plan, Probe};
use tokio::net::UnixStream;

use super::error::CallError;

/// Where a job's `Alive` and `Progress` events go. `Send`, so a call can run on a spawned task.
pub type EventSink<'a> = &'a mut (dyn FnMut(Event) + Send);

/// The only protocol version this client speaks.
pub const PROTOCOL_VERSION: u32 = 1;

/// One connection to a decoder runner. Every reply is hostile input: the shared codec caps a frame at 64 KiB
/// before it allocates and parses strictly, and this type adds the sequence rules (one response per request,
/// events only while a job runs).
///
/// A call that ends in anything but a typed decoder answer, and a call whose future is dropped before it
/// finishes, closes the connection; later calls then fail with `CallError::Disconnected`. Closing the socket
/// is also what cancels a running job on the runner. No call has a timeout of its own: the caller bounds it.
pub struct DecoderConn {
    live: Option<Live>,
}

struct Live {
    stream: UnixStream,
    reader: FrameReader,
}

/// `Read` over the bytes a tokio stream has ready, so the synchronous frame reader can be polled.
struct Ready<'a>(&'a UnixStream);

impl Read for Ready<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.try_read(buf)
    }
}

fn encode(request: Request) -> Result<Vec<u8>, CallError> {
    let mut frame = Vec::new();
    match write_message(&mut frame, &Message::Request(request)) {
        Ok(()) => Ok(frame),
        Err(WriteError::Invalid(violation)) => Err(CallError::InvalidRequest(violation)),
        Err(WriteError::Io(error)) => Err(CallError::Io(error)),
    }
}

fn out_of_sequence() -> CallError {
    CallError::Violation(ProtocolViolation(ViolationKind::OutOfSequence))
}

async fn write_all(stream: &UnixStream, mut bytes: &[u8]) -> Result<(), CallError> {
    while !bytes.is_empty() {
        stream.writable().await?;
        match stream.try_write(bytes) {
            Ok(0) => return Err(CallError::Closed),
            Ok(written) => bytes = &bytes[written..],
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Cancel safe: the reader keeps its partial frame between polls.
async fn next_reply(stream: &UnixStream, reader: &mut FrameReader) -> Result<Reply, CallError> {
    loop {
        match reader.poll_reply(&mut Ready(stream)) {
            Ok(Polled::Ready(reply)) => return Ok(reply),
            Ok(Polled::Closed) => return Err(CallError::Closed),
            Ok(Polled::Pending) => stream.readable().await?,
            Err(ReadError::Violation(violation)) => return Err(violation.into()),
            Err(ReadError::Io(error)) => return Err(error.into()),
        }
    }
}

impl Live {
    /// Sends `request` and returns the response. `events` is `None` outside a job, where an event is a
    /// violation. When `cancel` completes, `cancel` is sent once and the wait goes on: the job may still
    /// answer with its own result, because a cancel can lose that race.
    async fn run(
        &mut self,
        request: Vec<u8>,
        mut events: Option<EventSink<'_>>,
        cancel: impl Future<Output = ()>,
    ) -> Result<Response, CallError> {
        let Self { stream, reader } = self;
        write_all(stream, &request).await?;
        let mut cancel = std::pin::pin!(cancel);
        let mut cancel_sent = false;
        loop {
            let reply = tokio::select! {
                reply = next_reply(stream, reader) => reply?,
                () = &mut cancel, if !cancel_sent => {
                    write_all(stream, &encode(Request::Cancel)?).await?;
                    cancel_sent = true;
                    continue;
                }
            };
            match (reply, events.as_deref_mut()) {
                (Reply::Event(event), Some(sink)) => sink(event),
                (Reply::Event(_), None) => return Err(out_of_sequence()),
                (Reply::Response(Response::Cancelled), _) if !cancel_sent => {
                    return Err(out_of_sequence());
                }
                (Reply::Response(response), _) => return Ok(response),
            }
        }
    }
}

impl DecoderConn {
    /// Connects to the runner's socket. A refusal or a missing socket is an `io::Error` the caller backs off on.
    pub async fn connect(socket: &Path) -> io::Result<Self> {
        let stream = UnixStream::connect(socket).await?;
        Ok(Self {
            live: Some(Live {
                stream,
                reader: FrameReader::new(),
            }),
        })
    }

    pub fn is_open(&self) -> bool {
        self.live.is_some()
    }

    async fn exchange<T>(
        &mut self,
        request: Request,
        pick: fn(Response) -> Option<T>,
        events: Option<EventSink<'_>>,
        cancel: impl Future<Output = ()>,
    ) -> Result<T, CallError> {
        let frame = encode(request)?;
        let mut live = self.live.take().ok_or(CallError::Disconnected)?;
        let response = live.run(frame, events, cancel).await?;
        let (keep, result) = match response {
            Response::Error(DecodeError::ProtocolViolation) => (
                false,
                Err(CallError::Decode(DecodeError::ProtocolViolation)),
            ),
            Response::Error(error) => (true, Err(CallError::Decode(error))),
            Response::Cancelled => (true, Err(CallError::Cancelled)),
            other => match pick(other) {
                Some(value) => (true, Ok(value)),
                None => (false, Err(out_of_sequence())),
            },
        };
        if keep {
            self.live = Some(live);
        }
        result
    }

    async fn handshake<T>(
        &mut self,
        request: Request,
        pick: fn(Response) -> Option<T>,
    ) -> Result<T, CallError> {
        self.exchange(request, pick, None, future::pending()).await
    }

    /// `hello`; the capabilities must say protocol 1, anything else is `Decode(ProtocolMismatch)`.
    pub async fn hello(&mut self) -> Result<Capabilities, CallError> {
        let request = Request::Hello {
            protocol: PROTOCOL_VERSION,
        };
        let capabilities = self
            .handshake(request, |response| match response {
                Response::Capabilities(capabilities) => Some(capabilities),
                _ => None,
            })
            .await?;
        if capabilities.protocol == PROTOCOL_VERSION {
            Ok(capabilities)
        } else {
            self.live = None;
            Err(CallError::Decode(DecodeError::ProtocolMismatch))
        }
    }

    pub async fn self_check(
        &mut self,
        forbidden_targets: &[String],
    ) -> Result<SelfCheckReport, CallError> {
        let request = Request::SelfCheck {
            forbidden_targets: forbidden_targets.to_vec(),
        };
        self.handshake(request, |response| match response {
            Response::SelfCheck(report) => Some(report),
            _ => None,
        })
        .await
    }

    /// `probe` of the input under `job_dir`. `events` receives `Alive` and `Progress`; when `cancel`
    /// completes the runner is asked to stop the job and the call ends with `CallError::Cancelled`, or with
    /// the result when the job won the race.
    pub async fn probe(
        &mut self,
        job_dir: &str,
        events: EventSink<'_>,
        cancel: impl Future<Output = ()>,
    ) -> Result<Probe, CallError> {
        let request = Request::Probe {
            job_dir: job_dir.to_owned(),
        };
        self.exchange(
            request,
            |response| match response {
                Response::Probe(probe) => Some(probe),
                _ => None,
            },
            Some(events),
            cancel,
        )
        .await
    }

    pub async fn render_still(
        &mut self,
        job_dir: &str,
        max_edge: u32,
        poster_at_ms: Option<u64>,
        limits: Limits,
        events: EventSink<'_>,
        cancel: impl Future<Output = ()>,
    ) -> Result<ImageResult, CallError> {
        let request = Request::RenderStill {
            job_dir: job_dir.to_owned(),
            max_edge,
            poster_at_ms,
            limits,
        };
        self.exchange(
            request,
            |response| match response {
                Response::ImageResult(image) => Some(image),
                _ => None,
            },
            Some(events),
            cancel,
        )
        .await
    }

    pub async fn transcode(
        &mut self,
        job_dir: &str,
        plan: Plan,
        limits: Limits,
        events: EventSink<'_>,
        cancel: impl Future<Output = ()>,
    ) -> Result<TranscodeResult, CallError> {
        let request = Request::Transcode {
            job_dir: job_dir.to_owned(),
            plan,
            limits,
        };
        self.exchange(
            request,
            |response| match response {
                Response::TranscodeResult(result) => Some(result),
                _ => None,
            },
            Some(events),
            cancel,
        )
        .await
    }
}
