//! The stub daemon: a JSON-RPC server at the daemon's process boundary that answers every
//! `DaemonControl` method, by default the way `daemon_control.md` describes and per call
//! from a [`Script`], and records every request it receives.

mod connection;
mod endpoint;

pub use connection::MARKUP;

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use serde_json::Value;
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};

use crate::script::{Answer, EventSchedule, Script, ScriptError};
use crate::state::DefaultDaemon;
use endpoint::Listener;

#[derive(Debug, thiserror::Error)]
pub enum StubError {
    #[error(transparent)]
    Script(#[from] ScriptError),
    #[error("stub endpoint: {0}")]
    Io(#[from] io::Error),
}

/// One request exactly as received.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub method: String,
    pub params: Option<Value>,
    pub received_at: Instant,
}

pub(crate) struct Shared {
    pub(crate) daemon: DefaultDaemon,
    records: Mutex<Vec<Record>>,
    answers: Mutex<HashMap<String, VecDeque<Answer>>>,
    pub(crate) events: Option<EventSchedule>,
    pub(crate) stop_events: watch::Sender<bool>,
}

impl Shared {
    pub(crate) fn record(&self, method: &str, params: Option<Value>) {
        self.records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Record {
                method: method.to_owned(),
                params,
                received_at: Instant::now(),
            });
    }

    /// The next scripted answer of `method`, or `Default` once its queue is empty.
    pub(crate) fn next_answer(&self, method: &str) -> Answer {
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(method)
            .and_then(VecDeque::pop_front)
            .unwrap_or(Answer::Default)
    }
}

pub struct StubDaemon {
    shared: Arc<Shared>,
    endpoint: String,
    /// The private directory that holds the socket (Linux); removed on shutdown or drop.
    #[cfg(unix)]
    runtime_dir: std::path::PathBuf,
    accept: JoinHandle<()>,
}

impl StubDaemon {
    /// Starts the stub on a fresh endpoint.
    pub async fn start(script: Script) -> Result<Self, StubError> {
        Self::serve(script, Listener::bind()?)
    }

    /// Starts the stub on the named pipe the window connects to.
    #[cfg(windows)]
    pub async fn start_on_pipe(script: Script, pipe: String) -> Result<Self, StubError> {
        Self::serve(script, Listener::bind_named(pipe)?)
    }

    fn serve(script: Script, listener: Listener) -> Result<Self, StubError> {
        script.validate()?;
        let (stop_events, _) = watch::channel(false);
        let shared = Arc::new(Shared {
            daemon: DefaultDaemon::new(&script),
            records: Mutex::new(Vec::new()),
            answers: Mutex::new(
                script
                    .answers
                    .into_iter()
                    .map(|(method, answers)| (method, answers.into()))
                    .collect(),
            ),
            events: script.events,
            stop_events,
        });
        let endpoint = listener.endpoint().to_owned();
        #[cfg(unix)]
        let runtime_dir = listener.runtime_dir().to_path_buf();
        let accept = tokio::spawn(accept_loop(Arc::clone(&shared), listener));
        Ok(Self {
            shared,
            endpoint,
            #[cfg(unix)]
            runtime_dir,
            accept,
        })
    }

    /// The socket path or pipe name clients connect to.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The directory to give the window as `XDG_RUNTIME_DIR`, so that its own endpoint rule
    /// resolves to this stub.
    #[cfg(unix)]
    pub fn runtime_dir(&self) -> &std::path::Path {
        &self.runtime_dir
    }

    pub fn records(&self) -> Vec<Record> {
        self.shared
            .records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn records_of(&self, method: &str) -> Vec<Record> {
        self.records()
            .into_iter()
            .filter(|r| r.method == method)
            .collect()
    }

    pub fn count(&self, method: &str) -> usize {
        self.records_of(method).len()
    }

    /// Appends scripted answers to the queue of `method`.
    pub fn push_answers(&self, method: &str, answers: Vec<Answer>) {
        self.shared
            .answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(method.to_owned())
            .or_default()
            .extend(answers);
    }

    /// Ends every running event schedule; later subscribers get none either.
    pub fn stop_events(&self) {
        self.shared.stop_events.send_replace(true);
    }

    /// Stops serving and removes the endpoint.
    pub async fn shutdown(mut self) -> io::Result<()> {
        self.accept.abort();
        if let Err(joined) = (&mut self.accept).await
            && !joined.is_cancelled()
        {
            return Err(io::Error::other(joined));
        }
        self.remove_endpoint()
    }

    #[cfg(unix)]
    fn remove_endpoint(&self) -> io::Result<()> {
        match std::fs::remove_dir_all(&self.runtime_dir) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    #[cfg(windows)]
    fn remove_endpoint(&self) -> io::Result<()> {
        Ok(())
    }
}

/// A test that fails before `shutdown` still leaves nothing behind.
impl Drop for StubDaemon {
    fn drop(&mut self) {
        self.accept.abort();
        if let Err(error) = self.remove_endpoint() {
            eprintln!("stub: removing the endpoint failed: {error}");
        }
    }
}

async fn accept_loop(shared: Arc<Shared>, mut listener: Listener) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(stream) => {
                    connections.spawn(connection::serve(Arc::clone(&shared), stream));
                }
                Err(error) => {
                    eprintln!("stub: accepting a connection failed: {error}");
                    return;
                }
            },
            Some(joined) = connections.join_next() => {
                if let Err(joined) = joined
                    && joined.is_panic()
                {
                    eprintln!("stub: a connection task panicked: {joined}");
                }
            }
        }
    }
}
