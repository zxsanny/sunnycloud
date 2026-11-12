//! IT-10-18 and AC-3 of AZ-3856: a frozen main loop makes the process abort within
//! `decoder_watchdog`, idle and while a job runs. Real subprocesses, real `SIGUSR1`, real
//! `SIGABRT`: the idle case runs the `decoder-runner` binary; the busy case needs a job that
//! lasts (the binary has no executor yet), so the test re-runs this very test binary as the
//! child process, serving a scripted 30 s job through the library loop with the same wiring
//! as `main`. Built only with the `test-freeze` feature.
#![cfg(feature = "test-freeze")]
mod common;

use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::{Duration, Instant};

use common::{CONTROL_BYTES, Client, Received, Scripted, config, make_job, render_still_in};
use decoder_runner::{Heartbeat, Watchdog, disable_dumps, serve, socket, test_freeze};
use media_shared::decoder_protocol::{Event, Reply};
use tempfile::TempDir;

const SIGABRT: i32 = 6;
const WATCHDOG: Duration = Duration::from_secs(3);
const CHILD_SOCKET: &str = "DECODER_SELF_ABORT_SOCKET";
const CHILD_SCRATCH: &str = "DECODER_SELF_ABORT_SCRATCH";

struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            self.child.kill().expect("kill the leftover child");
            self.child.wait().expect("reap the leftover child");
        }
    }
}

fn wait_for_socket(socket: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while UnixStream::connect(socket).is_err() {
        assert!(Instant::now() < deadline, "the child never listened");
        thread::sleep(Duration::from_millis(20));
    }
}

fn send_sigusr1(child: &Child) {
    let status = Command::new("kill")
        .args(["-USR1", &child.id().to_string()])
        .status()
        .expect("run kill");
    assert!(status.success());
}

fn wait_for_exit(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return Some(status);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// The last counter bump is at most one 50 ms tick before the signal, so the abort comes
/// `WATCHDOG` after it, give or take the watchdog's poll; the NFR allows 1 s either way.
fn assert_aborted_on_time(child: &mut Child, signalled: Instant) {
    let status = wait_for_exit(child, 10 * WATCHDOG).expect("the process did not end");
    let elapsed = signalled.elapsed();

    assert_eq!(status.signal(), Some(SIGABRT), "exit status {status:?}");
    assert!(
        elapsed >= WATCHDOG - Duration::from_millis(500)
            && elapsed <= WATCHDOG + Duration::from_secs(1),
        "aborted {elapsed:?} after the signal, expected 3 s +/- 1 s"
    );
}

#[test]
fn ac3_idle_runner_binary_aborts_within_the_watchdog_after_sigusr1() {
    let dir = TempDir::new().expect("tempdir");
    let socket = dir.path().join("decoder.sock");
    let scratch = dir.path().join("scratch");
    make_job(&scratch, "job", CONTROL_BYTES);
    let child = Command::new(env!("CARGO_BIN_EXE_decoder-runner"))
        .env_clear()
        .env("SUNNYCLOUD_ENVIRONMENT", "development")
        .env("SUNNYCLOUD_DECODER_SOCKET_PATH", &socket)
        .env("SUNNYCLOUD_DECODER_SCRATCH_ROOT", &scratch)
        .env("SUNNYCLOUD_DECODER_ALIVE_INTERVAL", "500ms")
        .env("SUNNYCLOUD_DECODER_WATCHDOG", "3s")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn runner");
    let mut runner = ChildGuard { child };
    wait_for_socket(&socket);
    thread::sleep(Duration::from_millis(300));

    let signalled = Instant::now();
    send_sigusr1(&runner.child);
    assert_aborted_on_time(&mut runner.child, signalled);

    let mut log = String::new();
    runner
        .child
        .stdout
        .take()
        .expect("piped stdout")
        .read_to_string(&mut log)
        .expect("read the log");
    assert!(
        log.contains("test_freeze_main_loop: main loop blocked"),
        "{log}"
    );
}

#[test]
fn ac3_runner_aborts_within_the_watchdog_while_a_job_runs() {
    let dir = TempDir::new().expect("tempdir");
    let socket = dir.path().join("decoder.sock");
    let scratch = dir.path().join("scratch");
    let job_dir = make_job(&scratch, "job", CONTROL_BYTES);
    let child = Command::new(std::env::current_exe().expect("test binary path"))
        .args(["child_process_main", "--exact", "--nocapture"])
        .env(CHILD_SOCKET, &socket)
        .env(CHILD_SCRATCH, &scratch)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the child test process");
    let mut runner = ChildGuard { child };
    wait_for_socket(&socket);
    let mut client = Client::new(UnixStream::connect(&socket).expect("connect"));
    client.send(render_still_in(&job_dir));
    match client.recv(Duration::from_secs(5)) {
        Received::Reply(Reply::Event(Event::Alive)) => {}
        other => panic!("expected the job's first Alive event, got {other:?}"),
    }

    let signalled = Instant::now();
    send_sigusr1(&runner.child);
    assert_aborted_on_time(&mut runner.child, signalled);
}

/// The child process of the busy test. In a normal run the variables are absent and this test
/// has nothing to do.
#[test]
fn child_process_main() {
    let (Ok(socket_path), Ok(scratch)) =
        (std::env::var(CHILD_SOCKET), std::env::var(CHILD_SCRATCH))
    else {
        return;
    };
    let mut config = config(
        socket_path.clone().into(),
        scratch.into(),
        Duration::from_millis(500),
    );
    config.watchdog = WATCHDOG;
    disable_dumps().expect("disable core dumps");
    test_freeze::install().expect("install the freeze handler");
    let listener: UnixListener = socket::bind(Path::new(&socket_path), None).expect("bind");
    let heartbeat = Arc::new(Heartbeat::new());
    let _watchdog = Watchdog::spawn(Arc::clone(&heartbeat), config.watchdog).expect("watchdog");

    let exit = serve(
        &listener,
        &Arc::new(Scripted::new(Duration::from_secs(30))),
        &config,
        &AtomicBool::new(false),
        &heartbeat,
    );

    panic!("the runner loop ended without the watchdog abort: {exit:?}");
}
