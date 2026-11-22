//! IT-10-18: a frozen main loop makes the runner abort within `decoder_watchdog` (stub variant, built
//! with `test-freeze`; the control is the release image, which receives no signal).

use std::time::{Duration, Instant};

use media_shared::decoder_protocol::Response;

use crate::decoder::{Decoder, Flavor, Job, release_image, stub_image};
use crate::docker::wait_for;
use crate::host::remove_tree;
use crate::marker::Behaviour;
use crate::peer::{photo_limits, render};
use crate::refs::{pattern_jpeg, record, workdir};

const WAIT: Duration = Duration::from_secs(30);
const WATCHDOG: Duration = Duration::from_secs(3);
const SIGABRT_EXIT: i64 = 134;
const FREEZE_LOG: &str = "test_freeze_main_loop: main loop blocked";

/// Sends `SIGUSR1` and returns how long the container took to stop, with its exit code.
fn freeze(decoder: &Decoder) -> (i64, Duration) {
    let waiter = decoder.waiter();
    let signalled = Instant::now();
    decoder.signal_runner("USR1");
    let (code, stopped) = waiter
        .stopped_within(Duration::from_secs(10))
        .expect("the runner aborts");
    (code, stopped.saturating_duration_since(signalled))
}

fn assert_aborted(decoder: &Decoder, code: i64, took: Duration, case: &str) {
    assert_eq!(code, SIGABRT_EXIT, "{case}: SIGABRT");
    // The spec's bound is the watchdog (3 s) plus 1 s after the last counter bump; the signal is sent
    // from the host, so the docker CLI round trip is allowed on top.
    assert!(
        (WATCHDOG - Duration::from_millis(500)..=WATCHDOG + Duration::from_millis(1300))
            .contains(&took),
        "{case}: aborted {took:?} after the signal"
    );
    record(
        "IT-10-18",
        &format!("{case}: abort {took:?} after SIGUSR1, exit {code}"),
    );
    assert!(
        decoder.logs().contains(FREEZE_LOG),
        "{case}: the freeze is logged"
    );
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_18_an_idle_runner_with_a_frozen_main_loop_aborts_within_the_watchdog() {
    let decoder = Decoder::start(&stub_image(), Flavor::Interactive, &[]);
    let before = decoder.state();

    let (code, took) = freeze(&decoder);

    assert_aborted(&decoder, code, took, "idle");
    decoder.wait_restarted(&before, WAIT).hello();
    decoder.finish();
}

/// A runner with a hung tool, frozen by `SIGUSR1`: returns after the abort, the restart and `hello`.
fn abort_mid_job() -> (Decoder, Job) {
    let mut decoder = Decoder::start(&stub_image(), Flavor::Interactive, &[]);
    let before = decoder.state();
    let job = decoder.job(&Behaviour::Hang.jpeg());
    let mut peer = decoder.connect();
    peer.send(render(&job, 320, photo_limits()));
    wait_for("the hung tool", WAIT, || job.work().exists());

    let (code, took) = freeze(&decoder);

    assert_aborted(&decoder, code, took, "running job");
    decoder.wait_restarted(&before, WAIT).hello();
    (decoder, job)
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_18_a_runner_waiting_on_a_job_aborts_and_the_new_instance_answers() {
    let (decoder, job) = abort_mid_job();

    // `work/` of the interrupted job survives the abort and is the supervisor's to sweep (next test); it is
    // removed here, as that sweep would, so that the group check of AC-7 can look at everything else.
    remove_tree(&job.work(), &decoder.image);
    decoder.finish();
}

/// IT-10-18 (b), as decided by the owner on 2026-11-22: an abort runs no destructors, so the runner leaves `work/`
/// of the interrupted job, and the supervisor's start-up sweep (E-20, AC-71) removes it. The harness plays the
/// sweep here; what the runner owes is a new instance that serves the next job on the swept scratch.
#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_18_work_of_the_interrupted_job_is_left_for_the_supervisor_sweep_and_the_next_job_runs() {
    let (mut decoder, job) = abort_mid_job();
    let left = job.work().exists();

    remove_tree(&job.work(), &decoder.image);
    let samples = workdir();
    let next = decoder.job(&pattern_jpeg(samples.path(), "next.jpg", 640, 480));
    let answer = decoder
        .connect()
        .call(render(&next, 320, photo_limits()), WAIT);

    assert!(left, "the runner left work/ for the sweep");
    assert!(matches!(answer, Response::ImageResult(_)), "{answer:?}");
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_18_sigstop_does_not_abort_the_runner_because_it_stops_the_watchdog_too() {
    let decoder = Decoder::start(&stub_image(), Flavor::Interactive, &[]);
    let before = decoder.state();

    decoder.signal_runner("STOP");
    std::thread::sleep(WATCHDOG + Duration::from_secs(2));
    decoder.signal_runner("CONT");
    std::thread::sleep(WATCHDOG);

    let after = decoder.state();
    assert!(after.running);
    assert_eq!(
        (after.started_at, after.restart_count),
        (before.started_at, before.restart_count)
    );
    decoder.connect().hello();
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_18_control_the_release_runner_idles_past_the_watchdog_without_aborting() {
    let decoder = Decoder::start(&release_image(), Flavor::Interactive, &[]);
    let before = decoder.state();

    std::thread::sleep(WATCHDOG * 2);

    let after = decoder.state();
    assert_eq!(
        (after.started_at, after.restart_count),
        (before.started_at, before.restart_count)
    );
    let mut peer = decoder.connect();
    assert!(matches!(
        peer.call(
            media_shared::decoder_protocol::Request::Hello { protocol: 1 },
            WAIT
        ),
        Response::Capabilities(_)
    ));
    decoder.finish();
}
