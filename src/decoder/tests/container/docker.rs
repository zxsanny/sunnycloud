//! `docker` through `std::process::Command`, and the one wait the suite makes on the daemon's own API.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Output};
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::{Duration, Instant};

pub fn docker(args: &[&str]) -> Output {
    Command::new("docker")
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("cannot run docker {args:?}: {error}"))
}

/// The trimmed stdout of a `docker` call that must succeed.
pub fn docker_ok(args: &[&str]) -> String {
    let output = docker(args);
    assert!(
        output.status.success(),
        "docker {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// What `docker inspect` says about the state of a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub running: bool,
    pub started_at: String,
    pub restart_count: u64,
}

pub fn state_of(name: &str) -> State {
    let text = docker_ok(&[
        "inspect",
        "--format",
        "{{.State.Running}} {{.State.StartedAt}} {{.RestartCount}}",
        name,
    ]);
    let mut words = text.split_whitespace();
    let mut next = |what: &str| {
        words
            .next()
            .unwrap_or_else(|| panic!("no {what} in {text:?}"))
    };
    State {
        running: next("running") == "true",
        started_at: next("started_at").to_owned(),
        restart_count: next("restart_count").parse().expect("restart count"),
    }
}

/// One row of `docker top`: a process of the container as the host sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
}

/// The processes of a running container; `None` when it is not running (between a stop and the restart).
pub fn processes_of(name: &str) -> Option<Vec<Process>> {
    let output = docker(&["top", name, "-eo", "pid,ppid,comm"]);
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    Some(
        text.lines()
            .skip(1)
            .filter_map(|line| {
                let mut words = line.split_whitespace();
                Some(Process {
                    pid: words.next()?.parse().ok()?,
                    ppid: words.next()?.parse().ok()?,
                    comm: words.next()?.to_owned(),
                })
            })
            .collect(),
    )
}

/// The wait of the Docker API on a thread: the exit code of the next stop of the container and the moment it
/// was seen, so that a test can time "the runner exits within 5 s of the reply".
///
/// `docker wait` cannot be used for this: the CLI registers with the daemon some tens of milliseconds after it
/// starts (seconds on a loaded host), and a runner that exits at once is restarted by `--restart
/// unless-stopped` before that, so the late wait blocks for the next exit. This waiter calls
/// `POST /containers/{name}/wait` itself and returns only after the daemon has answered the headers, which it
/// does once the wait is registered: a stop after `start` returns is never missed.
pub struct Waiter {
    result: Receiver<(i64, Instant)>,
}

impl Waiter {
    pub fn start(name: &str) -> Self {
        let mut stream =
            UnixStream::connect(docker_socket()).expect("connect to the Docker socket");
        write!(
            stream,
            "POST /containers/{name}/wait?condition=not-running HTTP/1.0\r\nHost: docker\r\nContent-Length: 0\r\n\r\n"
        )
        .expect("send the wait request");
        let mut reader = BufReader::new(stream);
        let mut status = String::new();
        reader
            .read_line(&mut status)
            .expect("read the wait status line");
        assert!(
            status.contains(" 200 "),
            "the Docker wait answered {status:?}"
        );
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).expect("read a wait header");
            if header.trim().is_empty() {
                break;
            }
        }
        let (sender, result) = channel();
        thread::spawn(move || {
            let mut body = String::new();
            reader
                .read_to_string(&mut body)
                .expect("read the wait body");
            let code = status_code(&body);
            // The receiver is gone when the test already failed: nothing is left to tell.
            sender.send((code, Instant::now())).unwrap_or_default();
        });
        Self { result }
    }

    /// `(exit code, when)` of the stop, or `None` when the container did not stop within `within`.
    pub fn stopped_within(&self, within: Duration) -> Option<(i64, Instant)> {
        self.result.recv_timeout(within).ok()
    }
}

/// Polls `ready` every 20 ms until it holds; panics naming `what` after `within`.
pub fn wait_for(what: &str, within: Duration, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + within;
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

/// True when the container runs nothing but the init and the runner.
pub fn only_the_runner_runs(name: &str) -> bool {
    processes_of(name).is_some_and(|rows| {
        rows.iter().any(|row| row.comm == "decoder-runner")
            && rows
                .iter()
                .all(|row| row.comm == "decoder-runner" || row.comm == "docker-init")
    })
}

/// The unix socket of the Docker daemon: `DOCKER_HOST=unix://...` or the default path.
fn docker_socket() -> String {
    std::env::var("DOCKER_HOST")
        .ok()
        .and_then(|host| host.strip_prefix("unix://").map(str::to_owned))
        .unwrap_or_else(|| "/var/run/docker.sock".to_owned())
}

/// `StatusCode` of the wait answer `{"Error":null,"StatusCode":2}`, whatever the framing around it.
fn status_code(body: &str) -> i64 {
    let after = body
        .split("\"StatusCode\":")
        .nth(1)
        .unwrap_or_else(|| panic!("no StatusCode in the wait answer {body:?}"));
    let digits: String = after
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    digits
        .parse()
        .unwrap_or_else(|error| panic!("StatusCode {digits:?} of {body:?}: {error}"))
}
