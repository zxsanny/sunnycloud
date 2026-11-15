//! AZ-3798 AC-4: SIGINT during `scrub run` lets the running step return, starts no new step, calls `finish_run`
//! once and ends within 5 s. One test on purpose: it raises a real SIGINT at the test process.

mod scrub_support;
mod support;

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use contracts::Scrubber;
use scrub_support::{Scripted, as_service, step};
use support::Harness;

fn raise_sigint() {
    let status = std::process::Command::new("kill")
        .args(["-INT", &std::process::id().to_string()])
        .status()
        .expect("kill starts");
    assert!(status.success());
}

#[test]
fn sigint_stops_after_the_running_step_and_closes_the_run_once() {
    // Arrange: an endless run of 400 ms steps; the first step reports that it started.
    let (started_tx, started_rx) = mpsc::channel();
    let scrubber = Arc::new(
        Scripted::new((0..100).map(|_| step(1, 0, 0, false)).collect())
            .delay(Duration::from_millis(400))
            .on_step(move || started_tx.send(()).expect("the test is listening")),
    );
    let service = as_service(&scrubber);
    let harness = Harness::new()
        .real_handler()
        .service::<dyn Scrubber, _, _>(move |_| {
            let service = Arc::clone(&service);
            async move { Ok(service) }
        });
    let run = std::thread::spawn(move || harness.run(&["scrub", "run"]));
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the first step started");

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
    let (starts, steps, finishes) = scrubber.calls();
    assert_eq!((starts.len(), finishes), (1, 1));
    assert!((1..=2).contains(&steps), "{steps} steps");
    assert_eq!(out.exit, 130, "{}", out.stderr);
    assert!(out.stdout.contains("interrupted: true"), "{}", out.stdout);
}
