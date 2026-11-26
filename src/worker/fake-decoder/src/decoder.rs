//! FD itself: the listener, the state every connection shares, and the in-process API of the worker tests.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{Request, SelfCheckItem, SelfCheckReport};
use tokio::net::UnixListener;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::record::{Record, Recorder, Seen};
use crate::script::{Answer, Script, ScriptError};
use crate::serve;

/// A pause after a failed `accept`, so a persistent failure (no file descriptors left) does not spin.
const ACCEPT_RETRY: Duration = Duration::from_millis(50);

#[derive(Debug, thiserror::Error)]
pub enum FakeError {
    #[error(transparent)]
    Script(#[from] ScriptError),
    #[error("cannot open the record file {path}: {source}")]
    Record {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot bind the decoder socket: {0}")]
    Bind(#[source] io::Error),
    #[error("cannot {what}: {source}")]
    Io {
        what: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("the listener task ended: {0}")]
    Stopped(String),
}

pub struct Config {
    pub socket: PathBuf,
    /// The group of the socket file, as the real decoder's `socket_gid`; `None` keeps the process's group.
    pub socket_gid: Option<u32>,
    pub script: Script,
    pub record: Option<PathBuf>,
}

impl Config {
    /// The default script at `socket`, with no record file.
    pub fn new(socket: &Path) -> Self {
        Self {
            socket: socket.to_path_buf(),
            socket_gid: None,
            script: Script::default(),
            record: None,
        }
    }
}

pub(crate) struct State {
    pub(crate) script: Script,
    /// In-process only: every render and transcode answer waits this long first.
    pub(crate) delay: Duration,
    /// When each `cancel` arrived while a job request was being answered.
    pub(crate) cancels: Vec<Instant>,
}

#[derive(Clone)]
pub(crate) struct Shared {
    state: Arc<Mutex<State>>,
    pub(crate) recorder: Arc<Recorder>,
    pub(crate) in_flight: Arc<AtomicUsize>,
    pub(crate) most_in_flight: Arc<AtomicUsize>,
    outages: mpsc::UnboundedSender<Duration>,
    /// Bumped to close every live connection.
    closing: Arc<watch::Sender<u64>>,
    idle_outage_fired: Arc<AtomicBool>,
}

impl Shared {
    pub(crate) fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Asks the listener for an outage of `refuse`.
    pub(crate) fn outage(&self, refuse: Duration) {
        if self.outages.send(refuse).is_err() {
            eprintln!("fake decoder: the listener is gone, no outage");
        }
    }

    /// Called once a `self_check` answer is out: starts the script's idle outage the first time.
    pub(crate) fn self_check_answered(&self) {
        let Some(idle) = self.state().script.idle_outage else {
            return;
        };
        if self.idle_outage_fired.swap(true, Ordering::SeqCst) {
            return;
        }
        let shared = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(idle.after).await;
            shared.outage(idle.refuse);
        });
    }
}

fn bind(socket: &Path, gid: Option<u32>) -> Result<UnixListener, FakeError> {
    let listener = crate::socket::bind(socket, gid).map_err(FakeError::Bind)?;
    UnixListener::from_std(listener).map_err(|source| FakeError::Io {
        what: "register the decoder socket",
        source,
    })
}

enum Event {
    Accepted(io::Result<tokio::net::UnixStream>),
    Outage(Duration),
}

async fn listen(
    mut listener: UnixListener,
    socket: PathBuf,
    gid: Option<u32>,
    shared: Shared,
    mut outages: mpsc::UnboundedReceiver<Duration>,
) -> Result<(), FakeError> {
    let mut connections = 0;
    loop {
        let event = tokio::select! {
            accepted = listener.accept() => Event::Accepted(accepted.map(|(stream, _)| stream)),
            Some(refuse) = outages.recv() => Event::Outage(refuse),
        };
        match event {
            Event::Accepted(Ok(stream)) => {
                connections += 1;
                // Subscribed here, before any later outage can bump the generation.
                let closing = shared.closing.subscribe();
                tokio::spawn(serve::connection(
                    stream,
                    connections,
                    shared.clone(),
                    closing,
                ));
            }
            Event::Accepted(Err(error)) => {
                eprintln!("fake decoder: accept failed: {error}");
                tokio::time::sleep(ACCEPT_RETRY).await;
            }
            Event::Outage(refuse) => {
                let detail = format!("refusing connections for {} ms", refuse.as_millis());
                shared.recorder.event(0, "outage_start", &detail);
                drop(listener);
                shared.closing.send_modify(|generation| *generation += 1);
                tokio::time::sleep(refuse).await;
                listener = bind(&socket, gid)?;
                shared.recorder.event(0, "outage_end", "listening again");
            }
        }
    }
}

/// The fake decoder of harness H2 on a real Unix socket. Serves every connection at once (the real runner queues
/// the second one); the in-process tests script it through the methods below.
pub struct FakeDecoder {
    shared: Shared,
    listener: JoinHandle<Result<(), FakeError>>,
}

impl FakeDecoder {
    /// Listens at `socket` with [`Script::default`] until the test scripts otherwise. Needs a Tokio runtime; for
    /// in-process tests only: it panics when the socket cannot be bound ([`FakeDecoder::serve`] returns the error).
    pub fn start(socket: &Path) -> Self {
        Self::serve(Config::new(socket)).expect("the fake decoder binds its socket")
    }

    /// Binds the socket and serves on the current Tokio runtime.
    pub fn serve(config: Config) -> Result<Self, FakeError> {
        let recorder =
            Recorder::new(config.record.as_deref()).map_err(|source| FakeError::Record {
                path: config.record.clone().unwrap_or_default(),
                source,
            })?;
        let listener = bind(&config.socket, config.socket_gid)?;
        let (outages, outage_requests) = mpsc::unbounded_channel();
        let shared = Shared {
            state: Arc::new(Mutex::new(State {
                script: config.script,
                delay: Duration::ZERO,
                cancels: Vec::new(),
            })),
            recorder: Arc::new(recorder),
            in_flight: Arc::new(AtomicUsize::new(0)),
            most_in_flight: Arc::new(AtomicUsize::new(0)),
            outages,
            closing: Arc::new(watch::channel(0).0),
            idle_outage_fired: Arc::new(AtomicBool::new(false)),
        };
        let listener = tokio::spawn(listen(
            listener,
            config.socket,
            config.socket_gid,
            shared.clone(),
            outage_requests,
        ));
        Ok(Self { shared, listener })
    }

    /// Resolves only when the listener stops: a failed re-bind after an outage, or a panic.
    pub async fn stopped(self) -> Result<(), FakeError> {
        self.listener
            .await
            .map_err(|error| FakeError::Stopped(error.to_string()))?
    }

    fn script(&self, change: impl FnOnce(&mut Script)) -> &Self {
        change(&mut self.shared.state().script);
        self
    }

    pub fn probes_fall_back_to(&self, fallback: Answer) -> &Self {
        self.script(|script| script.probe.default = fallback)
    }

    pub fn probes(&self, answers: impl IntoIterator<Item = Answer>) -> &Self {
        self.script(|script| script.probe.queue.extend(answers))
    }

    pub fn stills_fall_back_to(&self, fallback: Answer) -> &Self {
        self.script(|script| script.render_still.default = fallback)
    }

    pub fn stills(&self, answers: impl IntoIterator<Item = Answer>) -> &Self {
        self.script(|script| script.render_still.queue.extend(answers))
    }

    pub fn transcodes(&self, answers: impl IntoIterator<Item = Answer>) -> &Self {
        self.script(|script| script.transcode.queue.extend(answers))
    }

    /// Every render and transcode takes this long to answer; a `cancel` meanwhile is answered `cancelled`.
    pub fn delay(&self, delay: Duration) -> &Self {
        self.shared.state().delay = delay;
        self
    }

    /// From now on `self_check` reports a failed `no_network` item.
    pub fn fail_self_checks(&self) -> &Self {
        self.script(|script| {
            script.self_check = SelfCheckReport(vec![SelfCheckItem {
                name: "no_network".into(),
                ok: false,
            }]);
        })
    }

    /// From now on `hello` reports this image build time (Unix seconds).
    pub fn built_at(&self, unix_seconds: u64) -> &Self {
        self.script(|script| script.hello.build_timestamp = unix_seconds)
    }

    /// From now on `hello` reports a successful NVENC test encode.
    pub fn report_nvenc_ready(&self) -> &Self {
        self.script(|script| script.hello.nvenc_probe_ok = true)
    }

    /// When each `cancel` reached the decoder during a job request.
    pub fn cancels(&self) -> Vec<Instant> {
        self.shared.state().cancels.clone()
    }

    /// Every received message, as the record file holds it.
    pub fn records(&self) -> Vec<Record> {
        self.shared.recorder.records()
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.shared.recorder.seen()
    }

    /// The job requests (not `hello`, `self_check` or `cancel`) in arrival order.
    pub fn requests(&self) -> Vec<Request> {
        self.seen().into_iter().map(|seen| seen.request).collect()
    }

    pub fn hellos(&self) -> usize {
        self.shared.recorder.hellos()
    }

    /// The most job requests that were being answered at the same moment.
    pub fn most_in_flight(&self) -> usize {
        self.shared.most_in_flight.load(Ordering::SeqCst)
    }

    /// The outage of the `outage` answer, now: every connection and the listener close, and FD binds again after
    /// `refuse` (connects are refused meanwhile).
    pub fn outage(&self, refuse: Duration) {
        self.shared.outage(refuse);
    }

    pub fn idle(&self) -> bool {
        self.shared.in_flight.load(Ordering::SeqCst) == 0
    }
}
