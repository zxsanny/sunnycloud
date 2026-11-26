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
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use sc_clock::SystemClock;
use sc_config::{ConfigSources, load_keys};
use sc_observability::LineSink;
use sunnycloud_server::{
    BoxFuture, Call, CliEnv, CliError, CliSettings, Commands, ConfigCheck, ContextBuilder, Handler,
    Outcome, ServeSettings, ServiceContext, Terminal, execute,
};
use tokio_util::sync::CancellationToken;

/// The command table of `operator_command_surface.md`: minimal valid arguments and the route each logs under.
pub const COMMANDS: &[(&str, &str)] = &[
    ("serve", "/cli/serve"),
    ("migrate", "/cli/migrate"),
    ("migrate run", "/cli/migrate"),
    ("migrate status", "/cli/migrate-status"),
    ("migrate revert --steps 2 --yes", "/cli/migrate-revert"),
    ("admin setup-token", "/cli/admin-setup-token"),
    ("admin create", "/cli/admin-create"),
    ("admin reset alice", "/cli/admin-reset"),
    ("admin promote alice", "/cli/admin-promote"),
    ("admin demote alice", "/cli/admin-demote"),
    (
        "admin invite --admin alice create --note n",
        "/cli/admin-invite-create",
    ),
    ("admin invite --admin alice list", "/cli/admin-invite-list"),
    (
        "admin invite --admin alice revoke inv-1",
        "/cli/admin-invite-revoke",
    ),
    ("admin user list", "/cli/admin-user-list"),
    (
        "admin user set-quota alice 10GiB",
        "/cli/admin-user-set-quota",
    ),
    ("admin user delete alice --yes", "/cli/admin-user-delete"),
    ("admin dev-seed", "/cli/admin-dev-seed"),
    (
        "admin audit export --since 2026-01-01T00:00:00Z",
        "/cli/admin-audit-export",
    ),
    ("admin epoch-bump --yes", "/cli/admin-epoch-bump"),
    (
        "admin restore --account alice --path /a/b.txt --at 2026-01-01T00:00:00Z --dry-run",
        "/cli/admin-restore",
    ),
    ("admin gc-run", "/cli/admin-gc-run"),
    ("key-check", "/cli/key-check"),
    ("admin keycheck-init", "/cli/admin-keycheck-init"),
    ("admin rewrap --from-key-id 1 --yes", "/cli/admin-rewrap"),
    ("scrub run --full --rate 50", "/cli/scrub-run"),
    ("scrub status", "/cli/scrub-status"),
    (
        "admin media-requeue --account alice --kind thumbnail",
        "/cli/admin-media-requeue",
    ),
    ("admin media-stats", "/cli/admin-media-stats"),
    (
        "doctor --pre-flight --backup-interval 24h --attest-backup",
        "/cli/doctor",
    ),
    ("healthcheck", "/cli/healthcheck"),
    ("openapi", "/cli/openapi"),
    ("version", "/cli/version"),
];

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

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

pub struct Harness {
    /// The secrets folder of [`Harness::real_handler`], removed after the run.
    config_dir: Option<PathBuf>,
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
            config_dir: None,
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

    /// The real binary's handler, with the configuration sources and the settings slot the real wiring registers.
    /// The sources hold what the keyring and invite commands bind (`ConfigCheck::Services`): a database URL and a
    /// master key file (secret files under a fresh folder), a public URL and a blob root. A test that needs other
    /// values registers its own `ConfigSources` after this call.
    pub fn real_handler(mut self) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sc-harness-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir is writable");
        std::fs::write(
            dir.join("db_api_url"),
            "postgres://api@127.0.0.1:9/sunnycloud",
        )
        .expect("secret file is writable");
        std::fs::write(
            dir.join("master_key_file"),
            dir.join("master.key").display().to_string(),
        )
        .expect("secret file is writable");
        let sources = Arc::new(ConfigSources {
            env: [
                (
                    "SUNNYCLOUD_PUBLIC_URL",
                    "https://cloud.example.org".to_owned(),
                ),
                (
                    "SUNNYCLOUD_BLOB_ROOT",
                    dir.join("blobs").display().to_string(),
                ),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
            secrets_dir: dir.clone(),
            ..ConfigSources::default()
        });
        self.config_dir = Some(dir);
        self.handler(Arc::new(Commands))
            .service::<ConfigSources, _, _>(move |_| {
                let sources = Arc::clone(&sources);
                async move { Ok(sources) }
            })
            .service::<ServeSettings, _, _>(|services| async move {
                let sources = services.get::<ConfigSources>().await?;
                Ok(load_keys::<ServeSettings>(
                    &sources,
                    &ConfigCheck::Server.keys(),
                )?)
            })
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
        if let Some(dir) = &self.config_dir {
            std::fs::remove_dir_all(dir).expect("the harness secrets folder is removable");
        }
        Captured {
            exit,
            stdout: stdout.text(),
            stderr: stderr.text(),
            log: locked(&log.0).clone(),
            calls: self.port.calls(),
        }
    }
}
