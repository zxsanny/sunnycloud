//! Shared helpers: a plain HTTP/1.1 GET and the real worker process.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
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
    let mut stream = TcpStream::connect(addr).expect("connects to the ops listener");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout is settable");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .expect("request is written");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response is read");
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("status line");
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default();
    (status, body)
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
    pub addr: SocketAddr,
    pub env: BTreeMap<String, String>,
}

impl Running {
    pub fn start(token: &str) -> Self {
        let port = free_port();
        let token = TokenFile::new(token);
        let env = env_for(port, &token);
        let child = worker_command(&env, &[])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the worker binary starts");
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let deadline = Instant::now() + Duration::from_secs(15);
        while TcpStream::connect(addr).is_err() {
            assert!(Instant::now() < deadline, "the ops listener never opened");
            sleep(Duration::from_millis(50));
        }
        Self {
            child: Some(child),
            _token: token,
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
            child.kill().expect("the worker can be killed");
            child.wait().expect("the killed worker is reaped");
        }
    }
}
