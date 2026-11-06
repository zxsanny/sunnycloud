//! The command test harness: runs any command through the library's entry point (`execute`, the one the binary
//! calls) with a recording owner port, captured streams and a captured log, and reports the exit code, stdout,
//! stderr, the log lines and the calls the port saw.
//!
//! Command tasks add their own ports to the context with [`Harness::service`] and their own handler with
//! [`Harness::handler`]; the default handler forwards every command to the [`RecordingPort`].

#![allow(dead_code)]

use std::ffi::OsString;
use std::future::Future;
use std::io::{self, Cursor, Write};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use sc_clock::SystemClock;
use sc_observability::LineSink;
use sunnycloud_server::{
    BoxFuture, Call, CliEnv, CliError, CliSettings, Commands, ContextBuilder, Handler, Outcome,
    ServiceContext, Terminal, execute,
};
use tokio_util::sync::CancellationToken;

pub fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

type Responder =
    Arc<dyn Fn(CancellationToken) -> BoxFuture<'static, Result<Outcome, CliError>> + Send + Sync>;

/// The recording owner port: notes the route of each call, then answers as scripted.
pub struct RecordingPort {
    calls: Mutex<Vec<String>>,
    responder: Responder,
}

impl RecordingPort {
    pub fn new(responder: Responder) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            responder,
        }
    }

    pub fn calls(&self) -> Vec<String> {
        locked(&self.calls).clone()
    }

    pub async fn call(&self, route: &str, cancel: CancellationToken) -> Result<Outcome, CliError> {
        locked(&self.calls).push(route.to_owned());
        (self.responder)(cancel).await
    }
}

/// A responder that answers at once with `make()`.
pub fn answer(make: impl Fn() -> Result<Outcome, CliError> + Send + Sync + 'static) -> Responder {
    Arc::new(move |_| {
        let result = make();
        Box::pin(async move { result })
    })
}

/// A responder that runs `body` with the cancellation token.
pub fn answer_with<F, Fut>(body: F) -> Responder
where
    F: Fn(CancellationToken) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Outcome, CliError>> + Send + 'static,
{
    Arc::new(move |cancel| Box::pin(body(cancel)))
}

/// Forwards every command to the [`RecordingPort`] registered in the context.
pub struct PortHandler;

impl Handler for PortHandler {
    fn run<'a>(&'a self, call: Call<'a>) -> BoxFuture<'a, Result<Outcome, CliError>> {
        Box::pin(async move {
            let port = call.services.get::<RecordingPort>().await?;
            port.call(call.command.route(), call.cancel).await
        })
    }
}

#[derive(Clone, Default)]
struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    fn text(&self) -> String {
        String::from_utf8(locked(&self.0).clone()).expect("captured stream is UTF-8")
    }
}

impl Write for SharedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        locked(&self.0).extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Default)]
struct CapturedLog(Arc<Mutex<Vec<String>>>);

impl LineSink for CapturedLog {
    fn write_line(&self, line: &str) -> io::Result<()> {
        locked(&self.0).push(line.to_owned());
        Ok(())
    }
}

pub struct Captured {
    pub exit: u8,
    pub stdout: String,
    pub stderr: String,
    pub log: Vec<String>,
    pub calls: Vec<String>,
}

pub struct Harness {
    port: Arc<RecordingPort>,
    builder: ContextBuilder,
    handler: Arc<dyn Handler>,
    settings: CliSettings,
    stdin: String,
    tty: bool,
}

impl Harness {
    /// Port answering with an empty success; stdin is not a TTY; configuration defaults (`log = text`,
    /// confirmation required).
    pub fn new() -> Self {
        let port = Arc::new(RecordingPort::new(answer(|| Ok(Outcome::success()))));
        Self {
            port,
            builder: ServiceContext::builder(),
            handler: Arc::new(PortHandler),
            settings: CliSettings::default(),
            stdin: String::new(),
            tty: false,
        }
    }

    pub fn responder(mut self, responder: Responder) -> Self {
        self.port = Arc::new(RecordingPort::new(responder));
        self
    }

    pub fn handler(mut self, handler: Arc<dyn Handler>) -> Self {
        self.handler = handler;
        self
    }

    /// The real binary's handler.
    pub fn real_handler(self) -> Self {
        self.handler(Arc::new(Commands))
    }

    pub fn settings(mut self, settings: CliSettings) -> Self {
        self.settings = settings;
        self
    }

    pub fn log_json(mut self) -> Self {
        self.settings.format = sc_observability::LogFormat::Json;
        self
    }

    pub fn confirm_required(mut self, required: bool) -> Self {
        self.settings.confirm_required = required;
        self
    }

    /// stdin is a TTY and the operator types `input`.
    pub fn tty(mut self, input: &str) -> Self {
        self.tty = true;
        self.stdin = input.to_owned();
        self
    }

    pub fn service<T, F, Fut>(mut self, build: F) -> Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(ServiceContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Arc<T>, CliError>> + Send + 'static,
    {
        self.builder = self.builder.register::<T, _, _>(build);
        self
    }

    pub fn port(&self) -> Arc<RecordingPort> {
        Arc::clone(&self.port)
    }

    pub fn run(self, args: &[&str]) -> Captured {
        let stdout = SharedBuffer::default();
        let stderr = SharedBuffer::default();
        let log = CapturedLog::default();
        let port = Arc::clone(&self.port);
        let settings = self.settings;
        let services = self
            .builder
            .register::<CliSettings, _, _>(move |_| async move { Ok(Arc::new(settings)) })
            .register::<RecordingPort, _, _>(move |_| {
                let port = Arc::clone(&port);
                async move { Ok(port) }
            })
            .finish();
        let env = CliEnv {
            terminal: Terminal::new(
                Box::new(Cursor::new(self.stdin.into_bytes())),
                self.tty,
                Box::new(stdout.clone()),
                Box::new(stderr.clone()),
            ),
            services,
            handler: self.handler,
            log_sink: Arc::new(log.clone()),
            clock: Arc::new(SystemClock::new()),
        };
        let argv: Vec<OsString> = std::iter::once("sunnycloud")
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect();
        let exit = execute(argv, &env);
        Captured {
            exit,
            stdout: stdout.text(),
            stderr: stderr.text(),
            log: locked(&log.0).clone(),
            calls: self.port.calls(),
        }
    }
}
