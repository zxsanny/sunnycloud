//! A fake decoder on a real Unix socket, written for the slot loop tests: it speaks decoder protocol v1 through
//! the shared codec, answers each job request from a script, plays the decoder by writing `out/result.<ext>`,
//! and records every request with the state of `out/` when it arrived.

use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{
    Capabilities, DecodeError, Event, ImageResult, Message, Request, Response, SelfCheckItem,
    SelfCheckReport, TranscodeResult, parse_request, write_message,
};
use media_shared::policy::Probe;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

pub fn capabilities() -> Capabilities {
    Capabilities {
        protocol: 1,
        vips: "8.15".into(),
        ffmpeg: "7.1".into(),
        libheif: "1.19".into(),
        build_timestamp: 1_700_000_000,
        nvenc_probe_ok: false,
        nvenc_sessions: 0,
    }
}

/// What the fake does with one job request.
#[derive(Debug, Clone)]
pub enum Answer {
    Probe(Probe),
    /// Writes `out/result.webp` holding the edge as text, then answers with these dimensions.
    Image {
        width: u32,
        height: u32,
    },
    /// Writes `out/result.mp4`, then answers with this probe of the output.
    Transcode(Probe),
    /// Answers `ImageResult` without writing a file.
    ImageWithoutFile,
    /// Writes a symbolic link as `out/result.webp`, then answers `ImageResult`.
    ImageAsSymlink,
    Error(DecodeError),
    /// Reads the request and closes the connection, as a runner that exits mid-job does.
    Hang,
    /// A hung runner: reads the request, sends no frame of any kind and ignores `cancel`.
    Silent,
    /// A runner that sends `Alive` at this interval but never answers the request and ignores `cancel`.
    AliveOnly(Duration),
    /// Writes these bytes instead of an answer, as a compromised runner does.
    Garbage(Vec<u8>),
}

/// The answers of one kind of request: the queue first, then the fallback for every request after it.
pub struct Queue {
    answers: VecDeque<Answer>,
    fallback: Answer,
}

impl Queue {
    fn next(&mut self) -> Answer {
        self.answers
            .pop_front()
            .unwrap_or_else(|| self.fallback.clone())
    }
}

#[derive(Debug, Clone)]
pub struct Seen {
    pub request: Request,
    /// When the request was taken.
    pub at: Instant,
    /// Entries of the job's `out/` when the request arrived.
    pub out_entries: usize,
}

struct Script {
    probes: Queue,
    stills: Queue,
    transcodes: Queue,
    seen: Vec<Seen>,
    hellos: usize,
    delay: Duration,
    /// When each `cancel` arrived while a job request was being answered.
    cancels: Vec<Instant>,
    self_check_ok: bool,
    nvenc_probe_ok: bool,
    build_timestamp: u64,
}

pub struct FakeDecoder {
    script: Arc<Mutex<Script>>,
    in_flight: Arc<AtomicUsize>,
    most_in_flight: Arc<AtomicUsize>,
}

impl FakeDecoder {
    /// Listens at `socket`. Until the test scripts otherwise, a probe answers `Unsupported` and every render
    /// and transcode answers `Corrupt`.
    pub fn start(socket: &Path) -> Self {
        let listener = UnixListener::bind(socket).expect("the fake decoder binds");
        let queue = |fallback| Queue {
            answers: VecDeque::new(),
            fallback,
        };
        let script = Arc::new(Mutex::new(Script {
            probes: queue(Answer::Error(DecodeError::Unsupported)),
            stills: queue(Answer::Error(DecodeError::Corrupt)),
            transcodes: queue(Answer::Error(DecodeError::Corrupt)),
            seen: Vec::new(),
            hellos: 0,
            delay: Duration::ZERO,
            cancels: Vec::new(),
            self_check_ok: true,
            nvenc_probe_ok: false,
            build_timestamp: capabilities().build_timestamp,
        }));
        let in_flight = Arc::new(AtomicUsize::new(0));
        let most_in_flight = Arc::new(AtomicUsize::new(0));
        let shared = Shared {
            script: Arc::clone(&script),
            in_flight: Arc::clone(&in_flight),
            most_in_flight: Arc::clone(&most_in_flight),
        };
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.expect("accept");
                tokio::spawn(serve(stream, shared.clone()));
            }
        });
        Self {
            script,
            in_flight,
            most_in_flight,
        }
    }

    fn script(&self) -> MutexGuard<'_, Script> {
        self.script.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn probes_fall_back_to(&self, fallback: Answer) -> &Self {
        self.script().probes.fallback = fallback;
        self
    }

    pub fn probes(&self, answers: impl IntoIterator<Item = Answer>) -> &Self {
        self.script().probes.answers.extend(answers);
        self
    }

    pub fn stills_fall_back_to(&self, fallback: Answer) -> &Self {
        self.script().stills.fallback = fallback;
        self
    }

    pub fn stills(&self, answers: impl IntoIterator<Item = Answer>) -> &Self {
        self.script().stills.answers.extend(answers);
        self
    }

    pub fn transcodes(&self, answers: impl IntoIterator<Item = Answer>) -> &Self {
        self.script().transcodes.answers.extend(answers);
        self
    }

    /// Every render and transcode takes this long to answer.
    pub fn delay(&self, delay: Duration) -> &Self {
        self.script().delay = delay;
        self
    }

    /// From now on `self_check` reports a failed item.
    pub fn fail_self_checks(&self) -> &Self {
        self.script().self_check_ok = false;
        self
    }

    /// From now on `hello` reports this image build time (Unix seconds).
    pub fn built_at(&self, unix_seconds: u64) -> &Self {
        self.script().build_timestamp = unix_seconds;
        self
    }

    /// From now on `hello` reports a successful NVENC test encode.
    pub fn report_nvenc_ready(&self) -> &Self {
        self.script().nvenc_probe_ok = true;
        self
    }

    /// When each `cancel` reached the decoder during a job request.
    pub fn cancels(&self) -> Vec<Instant> {
        self.script().cancels.clone()
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.script().seen.clone()
    }

    /// The job requests (not `hello` or `self_check`) in arrival order.
    pub fn requests(&self) -> Vec<Request> {
        self.seen().into_iter().map(|seen| seen.request).collect()
    }

    pub fn hellos(&self) -> usize {
        self.script().hellos
    }

    /// The most job requests that were being answered at the same moment.
    pub fn most_in_flight(&self) -> usize {
        self.most_in_flight.load(Ordering::SeqCst)
    }

    pub fn idle(&self) -> bool {
        self.in_flight.load(Ordering::SeqCst) == 0
    }
}

#[derive(Clone)]
struct Shared {
    script: Arc<Mutex<Script>>,
    in_flight: Arc<AtomicUsize>,
    most_in_flight: Arc<AtomicUsize>,
}

impl Shared {
    fn script(&self) -> MutexGuard<'_, Script> {
        self.script.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

async fn read_request(stream: &mut (impl AsyncRead + Unpin)) -> Option<Request> {
    let mut prefix = [0u8; 4];
    stream.read_exact(&mut prefix).await.ok()?;
    let mut payload = vec![0u8; u32::from_be_bytes(prefix) as usize];
    stream.read_exact(&mut payload).await.ok()?;
    Some(parse_request(&payload).expect("the supervisor sends valid requests"))
}

async fn write_bytes(stream: &mut OwnedWriteHalf, bytes: &[u8]) {
    if let Err(error) = stream.write_all(bytes).await {
        eprintln!("fake decoder: the client went away before the answer: {error}");
    }
}

async fn send(stream: &mut OwnedWriteHalf, message: Message) {
    let mut frame = Vec::new();
    write_message(&mut frame, &message).expect("a valid message");
    write_bytes(stream, &frame).await;
}

/// Requests reach `serve` through a channel, so a `cancel` can be seen while a job request is being answered.
async fn read_loop(mut stream: OwnedReadHalf, requests: mpsc::UnboundedSender<Request>) {
    while let Some(request) = read_request(&mut stream).await {
        if requests.send(request).is_err() {
            return;
        }
    }
}

type Requests = mpsc::UnboundedReceiver<Request>;

async fn serve(stream: UnixStream, shared: Shared) {
    let (read, mut write) = stream.into_split();
    let (requests_tx, mut requests) = mpsc::unbounded_channel();
    tokio::spawn(read_loop(read, requests_tx));
    while let Some(request) = requests.recv().await {
        let response = match &request {
            Request::Hello { .. } => {
                let mut script = shared.script();
                script.hellos += 1;
                Response::Capabilities(Capabilities {
                    nvenc_probe_ok: script.nvenc_probe_ok,
                    build_timestamp: script.build_timestamp,
                    ..capabilities()
                })
            }
            Request::SelfCheck { .. } => {
                let ok = shared.script().self_check_ok;
                Response::SelfCheck(SelfCheckReport(vec![SelfCheckItem {
                    name: "no_network".into(),
                    ok,
                }]))
            }
            Request::Cancel => Response::Cancelled,
            job => match run_job(&shared, job, &mut requests, &mut write).await {
                Some(response) => response,
                None => return,
            },
        };
        send(&mut write, Message::Response(response)).await;
    }
}

/// `None` closes the connection without an answer.
async fn run_job(
    shared: &Shared,
    request: &Request,
    requests: &mut Requests,
    write: &mut OwnedWriteHalf,
) -> Option<Response> {
    let (job_dir, answer, delay) = {
        let mut script = shared.script();
        let job_dir = match request {
            Request::Probe { job_dir }
            | Request::RenderStill { job_dir, .. }
            | Request::Transcode { job_dir, .. } => job_dir.clone(),
            other => panic!("not a job request: {other:?}"),
        };
        let out_entries =
            fs::read_dir(Path::new(&job_dir).join("out")).map_or(0, |entries| entries.count());
        script.seen.push(Seen {
            request: request.clone(),
            at: Instant::now(),
            out_entries,
        });
        let answer = match request {
            Request::Probe { .. } => script.probes.next(),
            Request::RenderStill { .. } => script.stills.next(),
            _ => script.transcodes.next(),
        };
        let delay = if matches!(request, Request::Probe { .. }) {
            Duration::ZERO
        } else {
            script.delay
        };
        (job_dir, answer, delay)
    };
    let now = shared.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
    shared.most_in_flight.fetch_max(now, Ordering::SeqCst);
    let out = work(shared, request, &job_dir, answer, delay, requests, write).await;
    shared.in_flight.fetch_sub(1, Ordering::SeqCst);
    out
}

/// Answers after `delay`, unless a `cancel` arrives first (answered `Cancelled`). The hung answers never end.
async fn work(
    shared: &Shared,
    request: &Request,
    job_dir: &str,
    answer: Answer,
    delay: Duration,
    requests: &mut Requests,
    write: &mut OwnedWriteHalf,
) -> Option<Response> {
    match answer {
        Answer::Silent => loop {
            note_cancel(shared, &requests.recv().await?);
        },
        Answer::AliveOnly(every) => loop {
            tokio::select! {
                () = tokio::time::sleep(every) => send(write, Message::Event(Event::Alive)).await,
                next = requests.recv() => {
                    note_cancel(shared, &next?);
                }
            }
        },
        Answer::Garbage(bytes) => {
            write_bytes(write, &bytes).await;
            loop {
                requests.recv().await?;
            }
        }
        answer => {
            if !delay.is_zero() {
                tokio::select! {
                    () = tokio::time::sleep(delay) => {}
                    next = requests.recv() => {
                        if note_cancel(shared, &next?) {
                            return Some(Response::Cancelled);
                        }
                    }
                }
            }
            respond(request, job_dir, answer)
        }
    }
}

/// Records a `cancel`; true when `request` was one.
fn note_cancel(shared: &Shared, request: &Request) -> bool {
    let cancel = matches!(request, Request::Cancel);
    if cancel {
        shared.script().cancels.push(Instant::now());
    }
    cancel
}

fn respond(request: &Request, job_dir: &str, answer: Answer) -> Option<Response> {
    let out = Path::new(job_dir).join("out");
    let response = match answer {
        Answer::Probe(probe) => Response::Probe(probe),
        Answer::Image { width, height } => {
            let Request::RenderStill { max_edge, .. } = request else {
                panic!("an image answer to {request:?}");
            };
            let bytes = format!("webp edge {max_edge}").into_bytes();
            fs::write(out.join("result.webp"), &bytes).expect("the fake writes its output");
            Response::ImageResult(ImageResult {
                width,
                height,
                bytes: bytes.len() as u64,
            })
        }
        Answer::Transcode(probe_of_output) => {
            let bytes = b"mp4 output".to_vec();
            fs::write(out.join("result.mp4"), &bytes).expect("the fake writes its output");
            Response::TranscodeResult(TranscodeResult {
                probe_of_output,
                bytes: bytes.len() as u64,
            })
        }
        Answer::ImageWithoutFile => Response::ImageResult(ImageResult {
            width: 320,
            height: 240,
            bytes: 1,
        }),
        Answer::ImageAsSymlink => {
            std::os::unix::fs::symlink("/etc/hostname", out.join("result.webp"))
                .expect("the fake plants a link");
            Response::ImageResult(ImageResult {
                width: 320,
                height: 240,
                bytes: 1,
            })
        }
        Answer::Error(error) => Response::Error(error),
        Answer::Hang => return None,
        Answer::Silent | Answer::AliveOnly(_) | Answer::Garbage(_) => {
            panic!("{answer:?} is handled before respond")
        }
    };
    Some(response)
}
