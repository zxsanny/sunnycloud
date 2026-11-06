//! AC-8: Ctrl-C is cooperative. This file holds one test on purpose: it raises a real SIGINT at the test
//! process, and a second concurrent test would receive it too.

mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use sc_error::ErrorCode;
use sunnycloud_server::{CliError, Exit};
use support::{Harness, answer_with};

struct Marks {
    token_seen: AtomicBool,
    port_returned: AtomicBool,
}

fn raise_sigint() {
    let status = std::process::Command::new("kill")
        .args(["-INT", &std::process::id().to_string()])
        .status()
        .expect("kill starts");
    assert!(status.success());
}

/// Runs `scrub run` against a port that signals `started`, waits for the token, takes `wind_down` to stop and returns.
fn long_command(wind_down: Duration) -> (Arc<Marks>, mpsc::Receiver<()>, thread::JoinHandle<u8>) {
    let marks = Arc::new(Marks {
        token_seen: AtomicBool::new(false),
        port_returned: AtomicBool::new(false),
    });
    let (started_tx, started_rx) = mpsc::channel();
    let port_marks = Arc::clone(&marks);
    let responder = answer_with(move |cancel| {
        let marks = Arc::clone(&port_marks);
        let started = started_tx.clone();
        async move {
            started.send(()).expect("test is listening");
            cancel.cancelled().await;
            marks.token_seen.store(true, Ordering::SeqCst);
            tokio::time::sleep(wind_down).await;
            marks.port_returned.store(true, Ordering::SeqCst);
            Err(CliError::Operator {
                code: ErrorCode::Internal.as_str(),
                detail: "cancelled".into(),
                exit: Exit::Failed,
            })
        }
    });
    let harness = Harness::new().responder(responder);
    (
        marks,
        started_rx,
        thread::spawn(move || harness.run(&["scrub", "run"]).exit),
    )
}

#[test]
fn sigint_triggers_the_token_and_the_process_waits_for_the_port() {
    // A port that honours the token ends within 5 s of the signal.
    let (marks, started, run) = long_command(Duration::from_millis(300));
    started
        .recv_timeout(Duration::from_secs(10))
        .expect("the port started");
    let signalled = Instant::now();
    raise_sigint();
    let exit = run.join().expect("run thread");

    assert!(
        signalled.elapsed() < Duration::from_secs(5),
        "took {:?}",
        signalled.elapsed()
    );
    assert!(
        marks.token_seen.load(Ordering::SeqCst),
        "the token was triggered"
    );
    assert!(
        marks.port_returned.load(Ordering::SeqCst),
        "the run ended only after the port returned"
    );
    assert_eq!(exit, 2);

    // A port that is slow to wind down is waited for, not abandoned.
    let (marks, started, run) = long_command(Duration::from_millis(900));
    started
        .recv_timeout(Duration::from_secs(10))
        .expect("the port started");
    raise_sigint();
    thread::sleep(Duration::from_millis(300));
    assert!(
        !run.is_finished(),
        "the process exited before the port returned"
    );
    let exit = run.join().expect("run thread");

    assert!(
        marks.token_seen.load(Ordering::SeqCst),
        "the token was triggered at once"
    );
    assert!(marks.port_returned.load(Ordering::SeqCst));
    assert_eq!(exit, 2);
}
