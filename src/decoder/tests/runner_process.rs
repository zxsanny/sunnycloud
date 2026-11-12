//! Process-level cases: the real binary, its environment and a protocol client.
//! AC-1, AC-2, AC-6, AC-8 and the honest report of the missing executors.
mod common;

use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use common::{CONTROL_BYTES, Client, Received, hello, make_job, render_still_in};
use media_shared::decoder_protocol::Request;
use media_shared::decoder_protocol::{DecodeError, Response};
use tempfile::TempDir;

const SECOND: Duration = Duration::from_secs(1);

fn command(socket: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_decoder-runner"));
    command
        .env_clear()
        .env("SUNNYCLOUD_ENVIRONMENT", "development")
        .env("SUNNYCLOUD_DECODER_SOCKET_PATH", socket);
    command
}

struct Runner {
    child: Child,
    socket: PathBuf,
    job_dir: PathBuf,
    _dir: TempDir,
}

impl Runner {
    fn start() -> Self {
        Self::start_with(|_| {})
    }

    fn start_with(edit: impl FnOnce(&mut Command)) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let socket = dir.path().join("decoder.sock");
        let scratch = dir.path().join("scratch");
        let job_dir = make_job(&scratch, "job", CONTROL_BYTES);
        let mut command = command(&socket);
        command.env("SUNNYCLOUD_DECODER_SCRATCH_ROOT", &scratch);
        edit(&mut command);
        let child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn runner");
        let runner = Self {
            child,
            socket,
            job_dir,
            _dir: dir,
        };
        runner.wait_for_socket();
        runner
    }

    fn wait_for_socket(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while UnixStream::connect(&self.socket).is_err() {
            assert!(Instant::now() < deadline, "runner never listened");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn connect(&self) -> Client {
        Client::new(UnixStream::connect(&self.socket).expect("connect"))
    }

    /// Stops the runner and returns what it logged.
    fn kill_and_read_log(&mut self) -> String {
        self.child.kill().expect("kill runner");
        self.child.wait().expect("reap runner");
        let mut log = String::new();
        self.child
            .stdout
            .take()
            .expect("piped stdout")
            .read_to_string(&mut log)
            .expect("read the log");
        log
    }

    fn exit_code_within(&mut self, limit: Duration) -> Option<i32> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return status.code();
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        if self.child.kill().is_ok() {
            self.child.wait().expect("reap runner");
        }
    }
}

/// A group other than the primary one when the user has one, so the check proves the chown.
fn target_group() -> u32 {
    let own = std::fs::metadata("/proc/self").expect("proc self").gid();
    let listed = Command::new("id").arg("-G").output().expect("id -G");
    String::from_utf8_lossy(&listed.stdout)
        .split_whitespace()
        .filter_map(|gid| gid.parse::<u32>().ok())
        .find(|gid| *gid != own)
        .unwrap_or(own)
}

fn is_capabilities(response: Response) -> bool {
    matches!(response, Response::Capabilities(_))
}

#[test]
fn ac1_socket_has_mode_0660_and_the_group_and_clients_are_served_in_turn() {
    let gid = target_group();
    let mut runner = Runner::start_with(|command| {
        command.env("SUNNYCLOUD_DECODER_GID", gid.to_string());
    });
    let metadata = std::fs::metadata(&runner.socket).expect("socket metadata");

    let mut first = runner.connect();
    first.send(hello());
    let first_ok = is_capabilities(first.expect_response(SECOND));
    let mut queued = runner.connect();
    queued.send(hello());
    let queued_silent = matches!(queued.recv(Duration::from_millis(300)), Received::Silence);
    drop(first);
    let queued_ok = is_capabilities(queued.expect_response(SECOND));
    drop(queued);
    let mut third = runner.connect();
    third.send(hello());
    let third_ok = is_capabilities(third.expect_response(SECOND));

    assert_eq!(metadata.permissions().mode() & 0o777, 0o660);
    assert_eq!(metadata.gid(), gid);
    assert!(first_ok && queued_ok && third_ok);
    assert!(
        queued_silent,
        "a second connection waits until the first closes"
    );
    assert_eq!(runner.exit_code_within(Duration::from_millis(100)), None);
}

#[test]
fn a_stale_socket_file_from_a_previous_run_is_replaced() {
    let dir = TempDir::new().expect("tempdir");
    let socket = dir.path().join("decoder.sock");
    drop(std::os::unix::net::UnixListener::bind(&socket).expect("make stale socket"));

    let mut child = command(&socket).spawn().expect("spawn");
    let deadline = Instant::now() + Duration::from_secs(10);
    while UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline, "runner never listened");
        thread::sleep(Duration::from_millis(20));
    }

    child.kill().expect("kill");
    child.wait().expect("reap");
}

#[test]
fn ac2_version_mismatch_is_clean_and_repeated_hello_is_answered() {
    let runner = Runner::start();
    let mut client = runner.connect();

    client.send(Request::Hello { protocol: 99 });
    let mismatch = client.expect_response(SECOND);
    let mut answers = Vec::new();
    for _ in 0..3 {
        client.send(hello());
        answers.push(client.expect_response(SECOND));
    }

    assert_eq!(mismatch, Response::Error(DecodeError::ProtocolMismatch));
    assert!(answers.into_iter().all(is_capabilities));
}

fn assert_violation_ends_the_runner(send: impl FnOnce(&mut Client)) {
    let mut runner = Runner::start();
    let mut client = runner.connect();

    send(&mut client);
    let reply = client.recv(SECOND);

    if let Received::Reply(media_shared::decoder_protocol::Reply::Response(response)) = reply {
        assert_eq!(response, Response::Error(DecodeError::ProtocolViolation));
    }
    assert_eq!(runner.exit_code_within(Duration::from_secs(5)), Some(2));
}

#[test]
fn ac6_frame_of_65537_bytes_ends_the_runner() {
    assert_violation_ends_the_runner(|client| {
        client.send_payload(&vec![b' '; 65_537]);
    });
}

#[test]
fn ac6_unknown_field_ends_the_runner() {
    assert_violation_ends_the_runner(|client| {
        client.send_payload(br#"{"request":{"hello":{"protocol":1,"extra":true}}}"#);
    });
}

#[test]
fn ac6_four_gib_length_prefix_ends_the_runner() {
    assert_violation_ends_the_runner(|client| {
        client.send_raw(&u32::MAX.to_be_bytes());
    });
}

fn run_to_completion(edit: impl FnOnce(&mut Command)) -> Output {
    let dir = TempDir::new().expect("tempdir");
    let mut command = command(&dir.path().join("decoder.sock"));
    edit(&mut command);
    command.output().expect("run runner")
}

#[test]
fn ac8_invalid_duration_and_unknown_variable_refuse_start_and_are_both_listed() {
    let output = run_to_completion(|command| {
        command
            .env("SUNNYCLOUD_DECODER_ALIVE_INTERVAL", "banana")
            .env("SUNNYCLOUD_NOT_A_KEY", "1");
    });

    let log = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{log}");
    assert!(log.contains("decoder_alive_interval"), "{log}");
    assert!(log.contains("SUNNYCLOUD_NOT_A_KEY"), "{log}");
    let line: serde_json::Value =
        serde_json::from_str(log.lines().next().expect("a log line")).expect("JSON line");
    assert_eq!(line["component"], "decoder-runner");
}

#[test]
fn jobs_are_reported_unsupported_and_self_check_fails_while_no_executor_exists() {
    let runner = Runner::start();
    let mut client = runner.connect();

    client.send(render_still_in(&runner.job_dir));
    let job = client.expect_response(SECOND);
    client.send(Request::SelfCheck {
        forbidden_targets: Vec::new(),
    });
    let check = client.expect_response(SECOND);

    assert_eq!(job, Response::Error(DecodeError::Unsupported));
    let Response::SelfCheck(report) = check else {
        panic!("expected a self-check report");
    };
    assert!(report.0.iter().all(|item| !item.ok));
}

#[test]
fn health_fails_without_a_socket_and_passes_with_one() {
    let down = run_to_completion(|command| {
        command.arg("health");
    });
    let runner = Runner::start();
    let up = command(&runner.socket)
        .arg("health")
        .output()
        .expect("health");

    assert_eq!(down.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&down.stdout).contains("socket_missing"));
    assert_eq!(up.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&up.stdout).contains("\"ok\""));
}

#[test]
fn hostile_inputs_are_refused_by_the_real_runner_without_reading_or_logging_their_target() {
    let mut runner = Runner::start();
    let scratch = runner.job_dir.parent().expect("scratch root").to_owned();
    let fifo_in = scratch.join("fifo/in");
    std::fs::create_dir_all(&fifo_in).expect("fifo job");
    assert!(
        Command::new("mkfifo")
            .arg(fifo_in.join("source"))
            .status()
            .expect("mkfifo")
            .success()
    );
    let link_in = scratch.join("link/in");
    std::fs::create_dir_all(&link_in).expect("link job");
    symlink("/etc/passwd", link_in.join("source")).expect("symlink");
    let mut client = runner.connect();

    let mut refusals = Vec::new();
    for job in ["fifo", "link"] {
        let sent = Instant::now();
        client.send(render_still_in(&scratch.join(job)));
        refusals.push((client.expect_response(SECOND), sent.elapsed()));
    }
    client.send(render_still_in(&runner.job_dir));
    let control = client.expect_response(SECOND);
    let log = runner.kill_and_read_log();

    for (response, elapsed) in refusals {
        assert_eq!(response, Response::Error(DecodeError::Corrupt));
        assert!(elapsed < SECOND, "{elapsed:?}");
    }
    assert_eq!(control, Response::Error(DecodeError::Unsupported));
    assert!(log.contains("job input refused"), "{log}");
    assert!(!log.contains("root:"), "{log}");
}

#[test]
fn the_runner_is_non_dumpable_so_an_abort_calls_no_core_handler() {
    let runner = Runner::start();

    let own = std::fs::metadata("/proc/self/status")
        .expect("own status")
        .uid();
    let runner_owner = std::fs::metadata(format!("/proc/{}/status", runner.child.id()))
        .expect("runner status")
        .uid();

    // The kernel hands the /proc files of a non-dumpable process to root.
    assert_eq!(runner_owner, 0, "the test process itself is owned by {own}");
}
