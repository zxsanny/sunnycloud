//! A fake decoder on a real Unix socket, written for the slot loop tests: it speaks decoder protocol v1 through
//! the shared codec, answers each job request from a script, plays the decoder by writing `out/result.<ext>`,
//! and records every request with the state of `out/` when it arrived.

use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use media_shared::decoder_protocol::{
    Capabilities, DecodeError, ImageResult, Message, Request, Response, SelfCheckItem,
    SelfCheckReport, TranscodeResult, parse_request, write_message,
};
use media_shared::policy::Probe;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

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

async fn read_request(stream: &mut UnixStream) -> Option<Request> {
    let mut prefix = [0u8; 4];
    stream.read_exact(&mut prefix).await.ok()?;
    let mut payload = vec![0u8; u32::from_be_bytes(prefix) as usize];
    stream.read_exact(&mut payload).await.ok()?;
    Some(parse_request(&payload).expect("the supervisor sends valid requests"))
}

async fn send(stream: &mut UnixStream, response: Response) {
    let mut frame = Vec::new();
    write_message(&mut frame, &Message::Response(response)).expect("a valid response");
    if let Err(error) = stream.write_all(&frame).await {
        eprintln!("fake decoder: the client went away before the answer: {error}");
    }
}

async fn serve(mut stream: UnixStream, shared: Shared) {
    while let Some(request) = read_request(&mut stream).await {
        let response = match &request {
            Request::Hello { .. } => {
                shared.script().hellos += 1;
                Response::Capabilities(capabilities())
            }
            Request::SelfCheck { .. } => {
                Response::SelfCheck(SelfCheckReport(vec![SelfCheckItem {
                    name: "no_network".into(),
                    ok: true,
                }]))
            }
            Request::Cancel => Response::Cancelled,
            job => match run_job(&shared, job).await {
                Some(response) => response,
                None => return,
            },
        };
        send(&mut stream, response).await;
    }
}

/// `None` closes the connection without an answer.
async fn run_job(shared: &Shared, request: &Request) -> Option<Response> {
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
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let response = respond(request, &job_dir, answer);
    shared.in_flight.fetch_sub(1, Ordering::SeqCst);
    response
}

fn respond(request: &Request, job_dir: &str, answer: Answer) -> Option<Response> {
    let out = Path::new(job_dir).join("out");
    match answer {
        Answer::Probe(probe) => Some(Response::Probe(probe)),
        Answer::Image { width, height } => {
            let Request::RenderStill { max_edge, .. } = request else {
                panic!("an image answer to {request:?}");
            };
            let bytes = format!("webp edge {max_edge}").into_bytes();
            fs::write(out.join("result.webp"), &bytes).expect("the fake writes its output");
            Some(Response::ImageResult(ImageResult {
                width,
                height,
                bytes: bytes.len() as u64,
            }))
        }
        Answer::Transcode(probe_of_output) => {
            let bytes = b"mp4 output".to_vec();
            fs::write(out.join("result.mp4"), &bytes).expect("the fake writes its output");
            Some(Response::TranscodeResult(TranscodeResult {
                probe_of_output,
                bytes: bytes.len() as u64,
            }))
        }
        Answer::ImageWithoutFile => Some(Response::ImageResult(ImageResult {
            width: 320,
            height: 240,
            bytes: 1,
        })),
        Answer::ImageAsSymlink => {
            std::os::unix::fs::symlink("/etc/hostname", out.join("result.webp"))
                .expect("the fake plants a link");
            Some(Response::ImageResult(ImageResult {
                width: 320,
                height: 240,
                bytes: 1,
            }))
        }
        Answer::Error(error) => Some(Response::Error(error)),
        Answer::Hang => None,
    }
}
