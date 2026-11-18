//! The real `sunnycloud serve` binary over the composition root's test stack (`root_support`): a PostgreSQL 18
//! clone, a blob volume and a master key file, the three listeners on free loopback ports and JSON logs. The
//! harness arranges every refusal from outside (superuser SQL, files); the binary is never told it is tested.

#![allow(dead_code)]

pub mod account;
pub mod http;

use std::io::{BufRead, BufReader, Read};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::root_support::Root;

pub const PROBE_EVERY: Duration = Duration::from_millis(100);
const PROBE_TIMEOUT: Duration = Duration::from_millis(50);

/// The start steps of `serve`, in their documented order (AC-1).
pub const START_STEPS: [&str; 10] = [
    "serve start: configuration",
    "serve start: database role",
    "serve start: schema version",
    "serve start: key check",
    "serve start: epoch marker",
    "serve start: services",
    "serve start: listeners",
    "serve start: late lists",
    "serve start: scheduler",
    "serve start: ready",
];

/// The shutdown steps that follow the signal, in their documented order (AC-5).
pub const STOP_STEPS: [&str; 8] = [
    "http shutdown: readiness 503",
    "http shutdown: listeners stopped accepting",
    "http shutdown: long-polls woken",
    "http shutdown: connections drained",
    "serve stop: scheduler stopped",
    "serve stop: upload checkpoints flushed",
    "serve stop: final group sync",
    "serve stop: pool closed",
];

#[derive(Debug, Clone, Copy)]
pub struct Ports {
    pub public: SocketAddr,
    pub internal: SocketAddr,
    pub ops: SocketAddr,
}

/// Every port this test process has handed out. The probe listener is closed before `serve` binds the port, and
/// the kernel may give that port to the next `bind(0)` of a parallel test; a port is therefore never handed out
/// twice in one process.
static HANDED_OUT: Mutex<Vec<u16>> = Mutex::new(Vec::new());

impl Ports {
    /// Three loopback ports free right now and not given to another test of this process.
    fn free() -> Ports {
        let mut handed_out = lock(&HANDED_OUT);
        // Listeners of refused ports stay open until the end, so the kernel offers other ports meanwhile.
        let mut refused = Vec::new();
        let mut chosen = Vec::new();
        while chosen.len() < 3 {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
            let addr = listener.local_addr().expect("bound address");
            if handed_out.contains(&addr.port()) {
                refused.push(listener);
            } else {
                handed_out.push(addr.port());
                chosen.push(addr);
            }
        }
        Ports {
            public: chosen[0],
            internal: chosen[1],
            ops: chosen[2],
        }
    }

    pub fn all(&self) -> [SocketAddr; 3] {
        [self.public, self.internal, self.ops]
    }
}

/// The stack `serve` starts on, valid as built.
pub struct Stack {
    pub root: Root,
    pub ports: Ports,
}

impl Stack {
    pub async fn new() -> Stack {
        let mut root = Root::new().await;
        let ports = Ports::free();
        root.setup
            .set("public_addr", &ports.public.to_string())
            .set("internal_addr", &ports.internal.to_string())
            .set("ops_addr", &ports.ops.to_string())
            .set("log", "json")
            .set("shutdown_grace", "3s")
            .set("max_holds_per_principal", "20")
            .set("max_open_sessions", "16")
            // No checkpoint before the shutdown: what the rows hold after it is the shutdown flush's work.
            .set("checkpoint_bytes", "64MiB");
        Stack { root, ports }
    }

    pub fn blob_root(&self) -> std::path::PathBuf {
        self.root.setup.dir.join("blobs")
    }

    pub fn spawn(&self) -> Server {
        Server::spawn(&self.root.setup.env)
    }

    /// Starts `serve` while every listener address is dialled every 100 ms, and waits for it to exit.
    pub fn refused(&self) -> Refused {
        let mut server = self.spawn();
        let done = Arc::new(AtomicBool::new(false));
        let accepted = Arc::new(AtomicBool::new(false));
        let prober = {
            let (done, accepted, addrs) =
                (Arc::clone(&done), Arc::clone(&accepted), self.ports.all());
            thread::spawn(move || {
                while !done.load(Ordering::Acquire) {
                    for addr in &addrs {
                        if TcpStream::connect_timeout(addr, PROBE_TIMEOUT).is_ok() {
                            accepted.store(true, Ordering::Release);
                        }
                    }
                    thread::sleep(PROBE_EVERY);
                }
            })
        };
        let exited = server.wait_exit(Duration::from_secs(30));
        done.store(true, Ordering::Release);
        prober.join().expect("the prober ends");
        Refused {
            exited,
            accepted: accepted.load(Ordering::Acquire),
        }
    }

    pub async fn finish(self) {
        self.root.finish().await;
    }
}

pub struct Refused {
    pub exited: Exited,
    pub accepted: bool,
}

/// One run of the binary; its stderr lines are collected as they come.
pub struct Server {
    child: Child,
    lines: Arc<Mutex<Vec<String>>>,
    reader: Option<JoinHandle<()>>,
    pub started: Instant,
}

pub struct Exited {
    pub code: Option<i32>,
    /// From the start of the process.
    pub elapsed: Duration,
    /// When the exit was seen.
    pub at: Instant,
    pub stderr: Vec<String>,
    pub stdout: String,
}

impl Exited {
    pub fn stderr_text(&self) -> String {
        self.stderr.join("\n")
    }

    /// The `msg` of every JSON log line, in order.
    pub fn messages(&self) -> Vec<String> {
        messages(&self.stderr)
    }
}

pub fn messages(lines: &[String]) -> Vec<String> {
    json_lines(lines)
        .iter()
        .filter_map(|line| line["msg"].as_str().map(str::to_owned))
        .collect()
}

pub fn json_lines(lines: &[String]) -> Vec<Value> {
    lines
        .iter()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}

/// The positions of `steps` in `messages`, each the first occurrence; panics naming a missing step.
pub fn positions(messages: &[String], steps: &[&str]) -> Vec<usize> {
    steps
        .iter()
        .map(|step| {
            messages
                .iter()
                .position(|message| message == step)
                .unwrap_or_else(|| panic!("`{step}` missing from {messages:#?}"))
        })
        .collect()
}

impl Server {
    fn spawn(env: &std::collections::BTreeMap<String, String>) -> Server {
        let started = Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_sunnycloud"))
            .arg("serve")
            .env_clear()
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary starts");
        let stderr = child.stderr.take().expect("stderr is piped");
        let lines = Arc::new(Mutex::new(Vec::new()));
        let reader = {
            let lines = Arc::clone(&lines);
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    match line {
                        Ok(line) => lock(&lines).push(line),
                        Err(error) => {
                            lock(&lines).push(format!("<stderr unreadable: {error}>"));
                            return;
                        }
                    }
                }
            })
        };
        Server {
            child,
            lines,
            reader: Some(reader),
            started,
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn lines(&self) -> Vec<String> {
        lock(&self.lines).clone()
    }

    /// Polls `GET /readyz` on the ops listener every 100 ms until 200; returns the time from the start.
    pub fn wait_ready(&mut self, ops: SocketAddr, within: Duration) -> Duration {
        while self.started.elapsed() < within {
            if let Ok(Some(status)) = self.child.try_wait() {
                panic!("serve exited with {status}:\n{}", self.lines().join("\n"));
            }
            if http::get_status(ops, "/readyz") == Some(200) {
                return self.started.elapsed();
            }
            thread::sleep(PROBE_EVERY);
        }
        panic!(
            "/readyz not 200 within {within:?}:\n{}",
            self.lines().join("\n")
        );
    }

    /// Sends `signal` (`TERM`, `INT`) to the process.
    pub fn signal(&self, signal: &str) -> Instant {
        let status = Command::new("kill")
            .args([&format!("-{signal}"), &self.pid().to_string()])
            .status()
            .expect("kill starts");
        assert!(status.success(), "kill -{signal} failed");
        Instant::now()
    }

    /// SIGKILL, a crash: no shutdown step runs. Returns the exit, which has no code (ended by the signal).
    pub fn crash(&mut self) -> Exited {
        self.signal("KILL");
        let exited = self.wait_exit(Duration::from_secs(10));
        assert_eq!(exited.code, None, "{}", exited.stderr_text());
        exited
    }

    /// Waits for the exit; past `within` the process is killed and the test fails.
    pub fn wait_exit(&mut self, within: Duration) -> Exited {
        let deadline = Instant::now() + within;
        let status = loop {
            match self.child.try_wait().expect("the child can be polled") {
                Some(status) => break status,
                None if Instant::now() >= deadline => {
                    self.child.kill().expect("the hung child can be killed");
                    panic!(
                        "serve did not exit within {within:?}:\n{}",
                        self.lines().join("\n")
                    );
                }
                None => thread::sleep(Duration::from_millis(20)),
            }
        };
        let at = Instant::now();
        let elapsed = at.duration_since(self.started);
        if let Some(reader) = self.reader.take() {
            reader.join().expect("the stderr reader ends");
        }
        let mut stdout = String::new();
        if let Some(mut out) = self.child.stdout.take() {
            out.read_to_string(&mut stdout).expect("stdout is UTF-8");
        }
        Exited {
            code: status.code(),
            elapsed,
            at,
            stderr: self.lines(),
            stdout,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            if let Err(error) = self.child.kill() {
                eprintln!("cannot kill serve {}: {error}", self.child.id());
            }
            if let Err(error) = self.child.wait() {
                eprintln!("cannot reap serve {}: {error}", self.child.id());
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
