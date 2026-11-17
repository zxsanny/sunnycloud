//! AZ-3797 AC-5: SIGINT during `admin rewrap` lets the running batch return, starts no new batch and ends within
//! 5 s. One test on purpose: it raises a real SIGINT at the test process.

mod key_support;
mod support;

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use contracts::MasterKeyring;
use key_support::{Scripted, as_service};
use support::{Harness, locked};

fn raise_sigint() {
    let status = std::process::Command::new("kill")
        .args(["-INT", &std::process::id().to_string()])
        .status()
        .expect("kill starts");
    assert!(status.success());
}

#[test]
fn sigint_stops_after_the_running_batch_and_starts_no_new_one() {
    // Arrange: an endless rewrap of 400 ms batches; the first batch reports that it started.
    let (started_tx, started_rx) = mpsc::channel();
    let keyring = Arc::new(
        Scripted::new()
            .batches(&[100; 100])
            .delay(Duration::from_millis(400))
            .on_batch(move || {
                started_tx.send(()).expect("the test is listening");
            }),
    );
    let service = as_service(&keyring);
    let harness = Harness::new()
        .real_handler()
        .service::<dyn MasterKeyring, _, _>(move |_| {
            let service = Arc::clone(&service);
            async move { Ok(service) }
        });
    let run = std::thread::spawn(move || {
        harness.run(&["admin", "rewrap", "--from-key-id", "1", "--yes"])
    });
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the first batch started");

    // Act
    let signalled = Instant::now();
    raise_sigint();
    let out = run.join().expect("run thread");

    // Assert
    assert!(
        signalled.elapsed() < Duration::from_secs(5),
        "{:?}",
        signalled.elapsed()
    );
    let calls = locked(&keyring.batch_calls).len();
    assert!((1..=2).contains(&calls), "{calls} batches");
    assert_eq!(out.exit, 130, "{}", out.stderr);
    assert!(out.stdout.contains("keep key 1"), "{}", out.stdout);
}
