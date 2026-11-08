//! Shared helpers: a plain HTTP/1.1 GET and the real worker process.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread::sleep;
use std::time::{Duration, Instant};

pub const TOKEN: &str = "unit-test-bearer-token";

/// A secret file in a private temporary folder, removed on drop. The loader reads secrets from files only.
pub struct TokenFile {
    dir: PathBuf,
    path: PathBuf,
}

impl TokenFile {
    pub fn new(token: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sunnycloud-worker-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("temporary folder");
        let path = dir.join("worker_api_token");
        fs::write(&path, token).expect("token file");
        Self { dir, path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TokenFile {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).expect("temporary folder is removed");
    }
}

/// `(status, body)` of one GET with `Connection: close`.
pub fn http_get(addr: SocketAddr, path: &str) -> (u16, String) {
    try_get(addr, path).expect("the GET is answered")
}

/// One GET; any failure to connect, send or read, or an answer without a status line, is an error.
fn try_get(addr: SocketAddr, path: &str) -> io::Result<(u16, String)> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no status line"))?;
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default();
    Ok((status, body))
}

/// Whether the worker answers `/healthz`. A bare TCP connect proves nothing: a port can accept a connection from
/// a socket that is not the worker's (a transient listener of another test, a connect to itself).
fn serves(addr: SocketAddr) -> bool {
    matches!(try_get(addr, "/healthz"), Ok((200, _)))
}

/// A listener on a port nothing else uses, held by the caller. It takes the lock `Running::start` holds, so it
/// cannot be given the port of a worker that is about to bind it.
pub fn busy_port() -> TcpListener {
    let _starting = STARTING.lock().unwrap_or_else(PoisonError::into_inner);
    TcpListener::bind("127.0.0.1:0").expect("binds")
}

pub fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port()
}

pub fn token_env(token: &TokenFile) -> BTreeMap<String, String> {
    BTreeMap::from([(
        "SUNNYCLOUD_WORKER_API_TOKEN_FILE".to_owned(),
        token.path().display().to_string(),
    )])
}

pub fn env_for(port: u16, token: &TokenFile) -> BTreeMap<String, String> {
    let mut env = token_env(token);
    env.extend([
        (
            "SUNNYCLOUD_WORKER_OPS_LISTEN".to_owned(),
            format!("127.0.0.1:{port}"),
        ),
        ("SUNNYCLOUD_LOG".to_owned(), "json".to_owned()),
    ]);
    env
}

pub fn worker_command(env: &BTreeMap<String, String>, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sunnycloud-worker"));
    command.args(args).env_clear().envs(env);
    command
}

/// The real worker process, stopped with SIGTERM by [`Running::stop`] and killed on drop.
pub struct Running {
    child: Option<Child>,
    _token: TokenFile,
    _keep: Box<dyn std::any::Any>,
    pub addr: SocketAddr,
    pub env: BTreeMap<String, String>,
}

/// A port that `free_port` returns is free again at once, so two tests starting workers together can be handed
/// the same one. Only one worker starts at a time: its port is held until the worker listens on it.
static STARTING: Mutex<()> = Mutex::new(());

impl Running {
    /// A worker with `extra` settings (decoder sockets, scratch volumes) on top of the ops address and the
    /// token; `keep` holds whatever those settings point at until the worker is gone. Returns once `/readyz`
    /// answers 200.
    pub fn start(
        token: &str,
        extra: BTreeMap<String, String>,
        keep: Box<dyn std::any::Any>,
    ) -> Self {
        let _starting = STARTING.lock().unwrap_or_else(PoisonError::into_inner);
        let port = free_port();
        let token = TokenFile::new(token);
        let mut env = env_for(port, &token);
        env.extend(extra);
        let mut child = worker_command(&env, &[])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the worker binary starts");
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let deadline = Instant::now() + Duration::from_secs(15);
        while !serves(addr) {
            assert!(
                Instant::now() < deadline,
                "the worker never answered /healthz"
            );
            let exited = child.try_wait().expect("the worker's state is readable");
            assert!(
                exited.is_none(),
                "the worker exited before it answered: {exited:?}"
            );
            sleep(Duration::from_millis(50));
        }
        while !matches!(try_get(addr, "/readyz"), Ok((200, _))) {
            assert!(Instant::now() < deadline, "the worker never became ready");
            sleep(Duration::from_millis(20));
        }
        Self {
            child: Some(child),
            _token: token,
            _keep: keep,
            addr,
            env,
        }
    }

    pub fn healthcheck(&self) -> Output {
        worker_command(&self.env, &["healthcheck"])
            .output()
            .expect("healthcheck runs")
    }

    /// SIGTERM, then the exit status and everything the process wrote.
    pub fn stop(&mut self) -> Output {
        let child = self.child.take().expect("not stopped yet");
        let status = Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .expect("kill runs");
        assert!(status.success(), "kill -TERM failed");
        child.wait_with_output().expect("the worker exits")
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let exited = child.try_wait().expect("the worker's state is readable");
            child.kill().expect("the worker can be killed");
            let output = child
                .wait_with_output()
                .expect("the killed worker is reaped");
            if std::thread::panicking() {
                eprintln!(
                    "worker at the end of a failed test: exited before the end {exited:?}, stdout {:?}, stderr {:?}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }
}
