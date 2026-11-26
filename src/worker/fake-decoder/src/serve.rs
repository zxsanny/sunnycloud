//! One connection: requests are read and recorded as they arrive, and answered from the script, so a `cancel` is
//! seen while a job request is being answered.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{
    DecodeError, Event, ImageResult, MAX_PAYLOAD, Message, ProtocolViolation, Request, Response,
    TranscodeResult, ViolationKind, parse_request, write_message,
};
use media_shared::policy::Probe;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, watch};
use tokio::time::{Interval, MissedTickBehavior};

use crate::decoder::Shared;
use crate::output::plant;
use crate::script::{Answer, Output};

/// The shortest `Alive` interval; a zero interval would be a busy loop.
const MIN_ALIVE: Duration = Duration::from_millis(1);

type Requests = mpsc::UnboundedReceiver<Request>;

/// Serves until the client closes, an answer closes, or `closing` changes (an outage).
pub(crate) async fn connection(
    stream: UnixStream,
    conn: u64,
    shared: Shared,
    mut closing: watch::Receiver<u64>,
) {
    let (read, write) = stream.into_split();
    let (sender, requests) = mpsc::unbounded_channel();
    let session = async {
        tokio::join!(
            read_loop(read, conn, &shared, sender),
            answer_loop(conn, &shared, requests, write)
        );
    };
    tokio::select! {
        () = session => {}
        _ = closing.changed() => {}
    }
}

enum Frame {
    Request(Request),
    Closed,
    Invalid(String),
}

async fn read_frame(stream: &mut OwnedReadHalf) -> Frame {
    let mut prefix = [0u8; 4];
    if let Err(error) = stream.read_exact(&mut prefix).await {
        return closed(&error);
    }
    let length = u32::from_be_bytes(prefix) as usize;
    if length > MAX_PAYLOAD {
        return Frame::Invalid(ProtocolViolation(ViolationKind::Oversize).to_string());
    }
    let mut payload = vec![0u8; length];
    if let Err(error) = stream.read_exact(&mut payload).await {
        return closed(&error);
    }
    match parse_request(&payload) {
        Ok(request) => Frame::Request(request),
        Err(violation) => Frame::Invalid(violation.to_string()),
    }
}

/// The end of the stream, at a frame boundary or inside a frame; any other read error is logged first.
fn closed(error: &std::io::Error) -> Frame {
    if error.kind() != std::io::ErrorKind::UnexpectedEof {
        eprintln!("fake decoder: read failed, closing the connection: {error}");
    }
    Frame::Closed
}

async fn read_loop(
    mut stream: OwnedReadHalf,
    conn: u64,
    shared: &Shared,
    requests: mpsc::UnboundedSender<Request>,
) {
    loop {
        match read_frame(&mut stream).await {
            Frame::Request(request) => {
                shared.recorder.request(conn, &request);
                if requests.send(request).is_err() {
                    return;
                }
            }
            Frame::Closed => return,
            Frame::Invalid(why) => {
                eprintln!("fake decoder: the client sent an invalid frame: {why}");
                shared.recorder.event(conn, "invalid", &why);
                return;
            }
        }
    }
}

async fn answer_loop(
    conn: u64,
    shared: &Shared,
    mut requests: Requests,
    mut write: OwnedWriteHalf,
) {
    while let Some(request) = requests.recv().await {
        let response = match &request {
            Request::Hello { .. } => Response::Capabilities(shared.state().script.hello.clone()),
            Request::SelfCheck { .. } => {
                Response::SelfCheck(shared.state().script.self_check.clone())
            }
            Request::Cancel => Response::Cancelled,
            Request::Probe { job_dir }
            | Request::RenderStill { job_dir, .. }
            | Request::Transcode { job_dir, .. } => {
                let job = Job {
                    conn,
                    shared,
                    request: &request,
                    job_dir,
                };
                match job.run(&mut requests, &mut write).await {
                    Some(response) => response,
                    None => return,
                }
            }
        };
        if !send(&mut write, &Message::Response(response)).await {
            return;
        }
        if matches!(request, Request::SelfCheck { .. }) {
            shared.self_check_answered();
        }
    }
}

/// False when nothing could be sent; the reason is logged.
async fn send(write: &mut OwnedWriteHalf, message: &Message) -> bool {
    let mut frame = Vec::new();
    if let Err(error) = write_message(&mut frame, message) {
        eprintln!("fake decoder: refused to send {message:?}: {error}");
        return false;
    }
    write_bytes(write, &frame).await
}

async fn write_bytes(write: &mut OwnedWriteHalf, bytes: &[u8]) -> bool {
    match write.write_all(bytes).await {
        Ok(()) => true,
        Err(error) => {
            eprintln!("fake decoder: the client went away before the answer: {error}");
            false
        }
    }
}

/// Counts a job request as answered while it lives, also when the connection is dropped mid-answer.
struct InFlight<'a>(&'a AtomicUsize);

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// An answer that ends in one response.
enum Ready {
    Probe(Probe),
    Image {
        width: u32,
        height: u32,
        output: Output,
    },
    Transcode {
        probe_of_output: Probe,
        output: Output,
    },
    Error(DecodeError),
}

enum Waited {
    Elapsed,
    Cancelled,
}

struct Job<'a> {
    conn: u64,
    shared: &'a Shared,
    request: &'a Request,
    job_dir: &'a str,
}

impl Job<'_> {
    /// `None` closes the connection without an answer.
    async fn run(&self, requests: &mut Requests, write: &mut OwnedWriteHalf) -> Option<Response> {
        let (answer, delay) = {
            let mut state = self.shared.state();
            match self.request {
                Request::Probe { .. } => (state.script.probe.next_answer(), Duration::ZERO),
                Request::RenderStill { .. } => {
                    (state.script.render_still.next_answer(), state.delay)
                }
                _ => (state.script.transcode.next_answer(), state.delay),
            }
        };
        let now = self.shared.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        let _in_flight = InFlight(&self.shared.in_flight);
        self.shared.most_in_flight.fetch_max(now, Ordering::SeqCst);
        self.work(answer, delay, requests, write).await
    }

    /// Plays `answer`. The hung answers never end on their own; they end when the client closes.
    async fn work(
        &self,
        mut answer: Answer,
        delay: Duration,
        requests: &mut Requests,
        write: &mut OwnedWriteHalf,
    ) -> Option<Response> {
        let ready = loop {
            answer = match answer {
                Answer::Probe(probe) => break Ready::Probe(probe),
                Answer::Image {
                    width,
                    height,
                    output,
                } => {
                    break Ready::Image {
                        width,
                        height,
                        output,
                    };
                }
                Answer::Transcode {
                    probe_of_output,
                    output,
                } => {
                    break Ready::Transcode {
                        probe_of_output,
                        output,
                    };
                }
                Answer::Error(error) => break Ready::Error(error),
                Answer::Hang => return None,
                Answer::Silent => loop {
                    self.note_cancel(&requests.recv().await?);
                },
                Answer::AliveOnly { every } => {
                    let mut alive = ticker(every);
                    loop {
                        tokio::select! {
                            _ = alive.tick() => {
                                if !send(write, &Message::Event(Event::Alive)).await {
                                    return None;
                                }
                            }
                            next = requests.recv() => {
                                self.note_cancel(&next?);
                            }
                        }
                    }
                }
                Answer::Garbage(bytes) => {
                    if !write_bytes(write, &bytes).await {
                        return None;
                    }
                    loop {
                        requests.recv().await?;
                    }
                }
                Answer::Outage { refuse } => {
                    self.shared.outage(refuse);
                    return None;
                }
                Answer::OverTime {
                    delay,
                    alive_every,
                    answer,
                } => match self.wait(delay, alive_every, requests, write).await? {
                    Waited::Cancelled => return Some(Response::Cancelled),
                    Waited::Elapsed => *answer,
                },
            };
        };
        if !delay.is_zero()
            && let Waited::Cancelled = self.wait(delay, None, requests, write).await?
        {
            return Some(Response::Cancelled);
        }
        self.respond(ready)
    }

    /// Waits `delay`, sending `Alive` every `alive_every` when set. `None` when the client went away.
    async fn wait(
        &self,
        delay: Duration,
        alive_every: Option<Duration>,
        requests: &mut Requests,
        write: &mut OwnedWriteHalf,
    ) -> Option<Waited> {
        let elapsed = tokio::time::sleep(delay);
        tokio::pin!(elapsed);
        let mut alive = alive_every.map(ticker);
        loop {
            tokio::select! {
                () = &mut elapsed => return Some(Waited::Elapsed),
                next = requests.recv() => {
                    if self.note_cancel(&next?) {
                        return Some(Waited::Cancelled);
                    }
                }
                () = tick(&mut alive) => {
                    if !send(write, &Message::Event(Event::Alive)).await {
                        return None;
                    }
                }
            }
        }
    }

    /// Records a `cancel`; true when `request` was one. Any other request during a job is not answered.
    fn note_cancel(&self, request: &Request) -> bool {
        let cancel = matches!(request, Request::Cancel);
        if cancel {
            self.shared.state().cancels.push(Instant::now());
        }
        cancel
    }

    /// Plants the output of an image or transcode answer. A file FD cannot plant is a broken harness, not a
    /// scripted fault: it is logged, recorded as `fd_error`, and the connection closes without an answer.
    fn respond(&self, ready: Ready) -> Option<Response> {
        let job = Path::new(self.job_dir);
        let (in_dir, out) = (job.join("in"), job.join("out"));
        let (path, planted) = match ready {
            Ready::Probe(probe) => return Some(Response::Probe(probe)),
            Ready::Error(error) => return Some(Response::Error(error)),
            Ready::Image {
                width,
                height,
                output,
            } => {
                let content = match self.request {
                    Request::RenderStill { max_edge, .. } => format!("webp edge {max_edge}"),
                    _ => "webp".to_owned(),
                };
                let path = out.join("result.webp");
                let planted = plant(&path, &output, content.as_bytes(), &in_dir).map(|bytes| {
                    Response::ImageResult(ImageResult {
                        width,
                        height,
                        bytes,
                    })
                });
                (path, planted)
            }
            Ready::Transcode {
                probe_of_output,
                output,
            } => {
                let path = out.join("result.mp4");
                let planted = plant(&path, &output, b"mp4 output", &in_dir).map(|bytes| {
                    Response::TranscodeResult(TranscodeResult {
                        probe_of_output,
                        bytes,
                    })
                });
                (path, planted)
            }
        };
        match planted {
            Ok(response) => Some(response),
            Err(error) => {
                let detail = format!("cannot plant {}: {error}", path.display());
                eprintln!("fake decoder: {detail}");
                self.shared.recorder.event(self.conn, "fd_error", &detail);
                None
            }
        }
    }
}

fn ticker(every: Duration) -> Interval {
    let every = every.max(MIN_ALIVE);
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    interval
}

/// The next tick, or never without a ticker.
async fn tick(alive: &mut Option<Interval>) {
    match alive {
        Some(interval) => {
            interval.tick().await;
        }
        None => std::future::pending().await,
    }
}
