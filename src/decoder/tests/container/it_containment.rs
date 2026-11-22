//! IT-10-14 to IT-10-16 and IT-10-19 (stub): a compromised child cannot outlive its job, read the
//! runner or the next job's input; hostile-class outcomes end the runner and Docker restarts it;
//! cancel and socket loss stop the job.

use std::fs;
use std::time::{Duration, Instant};

use media_shared::decoder_protocol::{DecodeError, LimitKind, Request, Response};

use crate::common::media::png_header_only;
use crate::decoder::{Decoder, Flavor, stub_image};
use crate::docker::{docker_ok, only_the_runner_runs, processes_of, wait_for};
use crate::marker::Behaviour;
use crate::peer::{greedy_limits, photo_limits, render};
use crate::refs::{pattern_jpeg, record, workdir};

const WAIT: Duration = Duration::from_secs(30);
const EXIT_VIOLATION: i64 = 2;
const EXIT_HOSTILE: i64 = 3;
const SMALL_MEMORY: [(&str, &str); 1] = [("SUNNYCLOUD_PHOTO_MEMORY", "128MiB")];

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_14_an_escaped_descendant_is_killed_before_the_reply_and_never_sees_the_next_job() {
    let work = workdir();
    let mut decoder = Decoder::start(&stub_image(), Flavor::Interactive, &[]);
    let before = decoder.state();
    let next_id = decoder.reserve_id();
    let hostile = decoder.job(&Behaviour::SetsidFork(next_id.clone()).jpeg());
    let waiter = decoder.waiter();
    let mut peer = decoder.connect();

    let reply = peer.call(render(&hostile, 320, greedy_limits()), WAIT);
    let replied = Instant::now();
    let rows = processes_of(&decoder.name);

    // A descendant found alive and killed at the end of the job is hostile-class (gap G-10-2).
    assert_eq!(reply, Response::Error(DecodeError::OutputInvalid));
    if let Some(rows) = rows {
        let strangers: Vec<_> = rows
            .iter()
            .filter(|r| r.comm != "decoder-runner" && r.comm != "docker-init")
            .collect();
        assert!(
            strangers.is_empty(),
            "descendants alive after the reply: {strangers:?}"
        );
    }
    let (code, stopped) = waiter
        .stopped_within(Duration::from_secs(5))
        .expect("the runner exits within 5 s");
    assert_eq!(code, EXIT_HOSTILE);
    assert!(stopped.saturating_duration_since(replied) <= Duration::from_secs(5));
    assert!(
        decoder
            .logs()
            .contains("a descendant of the runner was alive at the end of a job and was killed"),
        "the kill-all did not find the escaped descendant"
    );
    // The next job's input exists only now, after the kill-all.
    let next = decoder.job_named(&next_id, &pattern_jpeg(work.path(), "b.jpg", 800, 600));
    let mut again = decoder.wait_restarted(&before, WAIT);
    std::thread::sleep(Duration::from_secs(1));
    let leak = fs::read_to_string(decoder.scratch.join("leak.txt")).unwrap_or_default();
    assert!(
        !leak.contains("LEAK"),
        "a descendant read the next job's input"
    );
    let answer = again.call(render(&next, 320, photo_limits()), WAIT);
    assert!(matches!(answer, Response::ImageResult(_)), "{answer:?}");
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_14_a_child_cannot_write_the_next_jobs_input() {
    let work = workdir();
    let mut decoder = Decoder::start(&stub_image(), Flavor::Interactive, &[]);
    let next_id = decoder.reserve_id();
    let next = decoder.job_named(&next_id, &pattern_jpeg(work.path(), "b.jpg", 800, 600));
    let before = fs::read(next.source()).expect("read the next input");
    let child = decoder.job(&Behaviour::WriteNextJob(next_id).jpeg());
    let mut peer = decoder.connect();

    let reply = peer.call(render(&child, 320, photo_limits()), WAIT);

    assert_eq!(
        reply,
        Response::Error(DecodeError::Corrupt),
        "the stub exits 1"
    );
    let answer = fs::read_to_string(child.out("write-next.txt")).expect("the stub's answer");
    assert!(
        answer == "errno 13" || answer == "errno 1",
        "EACCES or EPERM, got {answer}"
    );
    assert_eq!(fs::read(next.source()).expect("read again"), before);
    decoder.finish();
}

fn errno_of(report: &str, name: &str) -> String {
    report
        .lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))
        .unwrap_or_else(|| panic!("no {name} in {report}"))
        .to_owned()
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_15_a_child_of_the_same_uid_cannot_ptrace_or_read_the_runner() {
    let mut decoder = Decoder::start(&stub_image(), Flavor::Interactive, &[]);
    let job = decoder.job(&Behaviour::Ptrace.jpeg());
    let mut peer = decoder.connect();

    let reply = peer.call(render(&job, 320, photo_limits()), WAIT);

    assert_eq!(
        reply,
        Response::Error(DecodeError::Corrupt),
        "the stub exits 1: not hostile-class"
    );
    let report = fs::read_to_string(job.out("ptrace.txt")).expect("the stub's report");
    // EPERM (1) or EACCES (13); ENOSYS (38) would mean the filter removes the call instead of denying it.
    for name in [
        "ptrace_attach",
        "process_vm_readv",
        "open_proc_mem",
        "open_proc_environ",
        "ptrace_traceme",
    ] {
        let answer = errno_of(&report, name);
        assert!(
            answer == "errno 1" || answer == "errno 13",
            "{name}: {answer}\n{report}"
        );
    }
    record("IT-10-15", &report.lines().collect::<Vec<_>>().join("; "));
    assert_eq!(errno_of(&report, "runner_Seccomp"), "2");
    assert_eq!(errno_of(&report, "runner_NoNewPrivs"), "1");
    decoder.finish();
}

/// One hostile-class case on a fresh container: the typed reply, the exit, the restart and `hello`.
fn hostile_case(
    env: &[(&str, &str)],
    source: Vec<u8>,
    expected: Response,
    exit_code: i64,
    name: &str,
) {
    let mut decoder = Decoder::start(&stub_image(), Flavor::Interactive, env);
    let before = decoder.state();
    let job = decoder.job(&source);
    let waiter = decoder.waiter();
    let mut peer = decoder.connect();

    let reply = peer.call(render(&job, 320, greedy_limits()), WAIT);
    let replied = Instant::now();
    let stopped = waiter.stopped_within(Duration::from_secs(5));

    assert_eq!(reply, expected, "{name}");
    let (code, at) =
        stopped.unwrap_or_else(|| panic!("{name}: the runner did not exit within 5 s"));
    assert_eq!(code, exit_code, "{name}");
    assert!(
        at.saturating_duration_since(replied) <= Duration::from_secs(5),
        "{name}"
    );
    let mut again = decoder.wait_restarted(&before, WAIT);
    again.hello();
    assert_ne!(
        decoder.state().started_at,
        before.started_at,
        "{name}: the start time changes"
    );
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_16_a_child_killed_by_sigsegv_ends_the_runner_after_the_reply() {
    hostile_case(
        &[],
        Behaviour::Segv.jpeg(),
        Response::Error(DecodeError::Corrupt),
        EXIT_HOSTILE,
        "SIGSEGV",
    );
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_16_a_memory_kill_ends_the_runner_after_the_reply() {
    let memory = Response::Error(DecodeError::LimitExceeded {
        limit: LimitKind::Memory,
    });
    hostile_case(
        &SMALL_MEMORY,
        Behaviour::Alloc(512).jpeg(),
        memory,
        EXIT_HOSTILE,
        "memory",
    );
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_16_a_pixel_limit_refusal_ends_the_runner_after_the_reply() {
    let pixels = Response::Error(DecodeError::LimitExceeded {
        limit: LimitKind::Pixels,
    });
    hostile_case(
        &[],
        png_header_only(30_000, 30_000),
        pixels,
        EXIT_HOSTILE,
        "pixels",
    );
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_16_a_peer_protocol_violation_ends_the_runner() {
    let decoder = Decoder::start(&stub_image(), Flavor::Interactive, &[]);
    let before = decoder.state();
    let waiter = decoder.waiter();
    let mut peer = decoder.connect();

    peer.send_payload(br#"{"request":"unheard_of"}"#);
    let stopped = waiter.stopped_within(Duration::from_secs(5));

    assert_eq!(stopped.map(|(code, _)| code), Some(EXIT_VIOLATION));
    decoder.wait_restarted(&before, WAIT).hello();
    decoder.finish();
}

#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_16_clean_failures_keep_the_same_container_serving() {
    let work = workdir();
    let good = pattern_jpeg(work.path(), "good.jpg", 800, 600);
    let mut decoder = Decoder::start(&stub_image(), Flavor::Interactive, &[]);
    let before = decoder.state();
    let truncated = decoder.job(&good[..100]);
    let svg = decoder.job(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
    let exit_three = decoder.job(&Behaviour::Exit(3).jpeg());
    let control = decoder.job(&good);
    let mut peer = decoder.connect();

    let corrupt = peer.call(render(&truncated, 320, photo_limits()), WAIT);
    let unsupported = peer.call(render(&svg, 320, photo_limits()), WAIT);
    let non_zero = peer.call(render(&exit_three, 320, photo_limits()), WAIT);
    let served = peer.call(render(&control, 320, photo_limits()), WAIT);

    assert_eq!(corrupt, Response::Error(DecodeError::Corrupt));
    assert!(
        matches!(
            unsupported,
            Response::Error(DecodeError::Unsupported | DecodeError::Corrupt)
        ),
        "{unsupported:?}"
    );
    assert_eq!(non_zero, Response::Error(DecodeError::Corrupt));
    assert!(matches!(served, Response::ImageResult(_)), "{served:?}");
    let after = decoder.state();
    assert_eq!(
        (after.started_at, after.restart_count),
        (before.started_at, before.restart_count)
    );
    decoder.finish();
}

/// IT-10-19 on a hung stub, for the photo path and the hostile tools; the same three steps with a real
/// transcode are `it_10_19_cancel_and_socket_loss_stop_a_real_transcode_and_the_runner_keeps_serving`.
#[test]
#[ignore = "needs DECODER_IMAGE and docker"]
fn it_10_19_cancel_and_socket_loss_stop_the_job_and_the_runner_keeps_serving() {
    let work = workdir();
    let mut decoder = Decoder::start(&stub_image(), Flavor::Interactive, &[]);
    let before = decoder.state();
    let cancelled = decoder.job(&Behaviour::Hang.jpeg());
    let lost = decoder.job(&Behaviour::Spin.jpeg());
    let control = decoder.job(&pattern_jpeg(work.path(), "c.jpg", 800, 600));
    let mut peer = decoder.connect();

    // (a) an explicit cancel
    peer.send(render(&cancelled, 320, photo_limits()));
    wait_for("the child of the first job", WAIT, || {
        cancelled.work().exists()
    });
    let cancel_sent = Instant::now();
    peer.send(Request::Cancel);
    let answer = peer.response(Duration::from_secs(2));
    let cancel_took = cancel_sent.elapsed();
    assert_eq!(answer, Response::Cancelled);
    assert!(cancel_took <= Duration::from_secs(2), "{cancel_took:?}");
    assert!(!cancelled.work().exists(), "work/ is removed by the cancel");
    assert!(
        only_the_runner_runs(&decoder.name),
        "{:?}",
        processes_of(&decoder.name)
    );

    // (b) the connection closes while a CPU-burning child runs
    peer.send(render(&lost, 320, photo_limits()));
    wait_for("the child of the second job", WAIT, || lost.work().exists());
    drop(peer);
    wait_for("the child to be killed", Duration::from_secs(2), || {
        only_the_runner_runs(&decoder.name)
    });
    wait_for("work/ to be removed", Duration::from_secs(2), || {
        !lost.work().exists()
    });
    let cpu = docker_ok(&[
        "stats",
        "--no-stream",
        "--format",
        "{{.CPUPerc}}",
        &decoder.name,
    ]);
    let percent: f64 = cpu.trim_end_matches('%').parse().expect("cpu percent");
    assert!(
        percent < 25.0,
        "the container is not idle after the socket loss: {cpu}"
    );

    // (c) a new connection and a new job, with no restart: the socket loss is not hostile-class
    let mut again = decoder.connect();
    let served = again.call(render(&control, 320, photo_limits()), WAIT);
    assert!(matches!(served, Response::ImageResult(_)), "{served:?}");
    let after = decoder.state();
    assert_eq!(
        (after.started_at, after.restart_count),
        (before.started_at, before.restart_count)
    );
    decoder.finish();
}
