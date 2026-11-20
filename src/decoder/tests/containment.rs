//! AZ-3855 through the real runner binary: kill-all after every job, `work/` removed on every
//! path, hostile-class outcomes end the runner after the typed reply, clean failures do not.
//! `vipsthumbnail` is a stub that only creates hostile behaviour; everything else is real.
mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::media::{
    Media, MediaRunner, WAIT, image_result, limits, path_with_stub, render, wait_until_gone,
};
use media_shared::decoder_protocol::{DecodeError, LimitKind, Request, Response};

const EXIT_HOSTILE: i32 = 3;
const FIVE_SECONDS: Duration = Duration::from_secs(5);

fn photo() -> Vec<u8> {
    Media::new().image("photo.jpg", 400, 300)
}

fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn hostile_runner(stub: &str) -> (tempfile::TempDir, MediaRunner) {
    let (bin, path) = path_with_stub(stub);
    let runner = MediaRunner::start_with(
        &[("SUNNYCLOUD_DECODER_MEMORY_POLL_INTERVAL", "50ms")],
        &path,
    );
    (bin, runner)
}

#[test]
fn a_descendant_that_left_its_process_group_is_killed_before_the_reply_and_the_runner_ends() {
    let (_bin, mut runner) =
        hostile_runner("setsid -f sh -c 'trap \"\" TERM HUP; exec sleep 59.401'");
    let mut client = runner.connect();
    let job = runner.job(&photo());

    let response = render(&mut client, &job, 320, limits());

    assert_eq!(response, Response::Error(DecodeError::OutputInvalid));
    wait_until_gone(&["59.401"]);
    assert_eq!(runner.exit_code_within(FIVE_SECONDS), Some(EXIT_HOSTILE));
}

#[test]
fn every_hostile_class_outcome_is_answered_and_then_ends_the_runner() {
    let cases: [(&str, &str, Response); 3] = [
        (
            "a crash by signal",
            "kill -SEGV $$",
            Response::Error(DecodeError::Corrupt),
        ),
        (
            "the wall limit",
            "exec sleep 59.402",
            Response::Error(DecodeError::LimitExceeded {
                limit: LimitKind::Wall,
            }),
        ),
        (
            "the memory limit",
            "dd if=/dev/zero bs=700M count=1 | sleep 59.403",
            Response::Error(DecodeError::LimitExceeded {
                limit: LimitKind::Memory,
            }),
        ),
    ];

    for (name, stub, expected) in cases {
        let (_bin, mut runner) = hostile_runner(stub);
        let mut client = runner.connect();
        let job = runner.job(&photo());
        let mut small = limits();
        small.wall = 800;
        small.memory_bytes = 128 << 20;

        let response = render(&mut client, &job, 320, small);

        assert_eq!(response, expected, "{name}");
        assert_eq!(
            runner.exit_code_within(FIVE_SECONDS),
            Some(EXIT_HOSTILE),
            "{name}"
        );
        wait_until_gone(&["59.402", "59.403"]);
    }
}

#[test]
fn a_pixel_limit_refusal_is_hostile_class_too() {
    let mut runner = MediaRunner::start_with(
        &[("SUNNYCLOUD_PHOTO_MAX_PIXELS", "10000")],
        &std::env::var("PATH").expect("PATH"),
    );
    let mut client = runner.connect();
    let job = runner.job(&Media::new().image("big.png", 200, 200));

    let response = render(&mut client, &job, 320, limits());

    assert_eq!(
        response,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Pixels
        })
    );
    assert_eq!(runner.exit_code_within(FIVE_SECONDS), Some(EXIT_HOSTILE));
}

#[test]
fn clean_failures_keep_the_same_runner_serving() {
    let media = Media::new();
    let mut runner = MediaRunner::start();
    let mut client = runner.connect();
    let truncated = runner.job(&media.image("photo.jpg", 800, 600)[..100]);
    let svg = runner.job(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
    let good = runner.job(&media.image("good.jpg", 800, 600));

    let corrupt = render(&mut client, &truncated, 320, limits());
    let unsupported = render(&mut client, &svg, 320, limits());
    let rendered = render(&mut client, &good, 320, limits());

    assert_eq!(corrupt, Response::Error(DecodeError::Corrupt));
    assert_eq!(unsupported, Response::Error(DecodeError::Unsupported));
    assert_eq!(image_result(rendered).width, 320);
    assert_eq!(runner.exit_code_within(Duration::from_millis(300)), None);
}

fn work_of(job: &Path) -> std::path::PathBuf {
    job.join("work")
}

#[test]
fn work_is_removed_after_a_success_a_failure_and_a_kill() {
    let media = Media::new();
    let good = {
        let mut runner = MediaRunner::start();
        let mut client = runner.connect();
        let job = runner.job(&media.image("photo.jpg", 800, 600));
        let rendered = render(&mut client, &job, 320, limits());
        (
            matches!(rendered, Response::ImageResult(_)),
            work_of(&job).exists(),
        )
    };
    let corrupt = {
        let mut runner = MediaRunner::start();
        let mut client = runner.connect();
        let job = runner.job(&media.image("photo.jpg", 800, 600)[..100]);
        render(&mut client, &job, 320, limits());
        work_of(&job).exists()
    };
    let killed = {
        let (_bin, mut runner) = hostile_runner("exec sleep 59.404");
        let mut client = runner.connect();
        let job = runner.job(&photo());
        let mut small = limits();
        small.wall = 500;
        render(&mut client, &job, 320, small);
        work_of(&job).exists()
    };

    assert_eq!(good, (true, false));
    assert!(!corrupt);
    assert!(!killed);
}

#[test]
fn work_is_removed_after_a_cancel_and_after_the_connection_is_lost() {
    let (_bin, mut runner) = hostile_runner("exec sleep 59.405");
    let mut client = runner.connect();
    let cancelled = runner.job(&photo());
    client.send(Request::RenderStill {
        job_dir: cancelled.to_str().expect("utf8").into(),
        max_edge: 320,
        poster_at_ms: None,
        limits: limits(),
    });
    wait_for("the child to start", || work_of(&cancelled).exists());

    client.send(Request::Cancel);
    let answer = client.expect_response(WAIT);

    assert_eq!(answer, Response::Cancelled);
    assert!(!work_of(&cancelled).exists());
    wait_until_gone(&["59.405"]);

    let (_bin, mut runner) = hostile_runner("exec sleep 59.406");
    let mut client = runner.connect();
    let lost = runner.job(&photo());
    client.send(Request::RenderStill {
        job_dir: lost.to_str().expect("utf8").into(),
        max_edge: 320,
        poster_at_ms: None,
        limits: limits(),
    });
    wait_for("the child to start", || work_of(&lost).exists());

    drop(client);

    wait_for("work/ to be removed", || !work_of(&lost).exists());
    wait_until_gone(&["59.406"]);
}
