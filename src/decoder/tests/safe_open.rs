//! Safe open of `job_dir` and `in/source` through the socket loop (IT-10-17, AC-25): each hostile
//! input is answered `Corrupt` within a second, the handler never runs for it, and a valid
//! control job runs after each. The character device case has no job-level test because
//! creating a device node needs a privilege; the unit test in `safe_open.rs` opens the real
//! `/dev/null` through the same walk.
mod common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use common::{CONTROL_BYTES, Client, Fixture, Scripted, make_job, render_still_in};
use media_shared::decoder_protocol::{DecodeError, ImageResult, Request, Response};
use tempfile::TempDir;

const SECOND: Duration = Duration::from_secs(1);

fn fixture() -> Fixture {
    Fixture::start(
        Scripted::new(Duration::from_millis(10)),
        Duration::from_secs(30),
    )
}

/// A job directory with `in/` and nothing in it; returns `in/`.
fn job_without_source(scratch: &Path, name: &str) -> std::path::PathBuf {
    let input = scratch.join(name).join("in");
    fs::create_dir_all(&input).expect("create in/");
    input
}

fn send_expecting_corrupt(client: &mut Client, request: Request) {
    let sent = Instant::now();
    client.send(request);

    let response = client.expect_response(SECOND);

    assert_eq!(response, Response::Error(DecodeError::Corrupt));
    assert!(sent.elapsed() < SECOND, "refusal took {:?}", sent.elapsed());
}

fn assert_control_job_runs(fixture: &Fixture, client: &mut Client, expected_jobs: usize) {
    client.send(fixture.render_still());

    let response = client.expect_response(SECOND);

    assert_eq!(
        response,
        Response::ImageResult(ImageResult {
            width: 1,
            height: 1,
            bytes: 1
        })
    );
    let inputs = fixture.handler.inputs.lock().expect("inputs");
    assert_eq!(
        inputs.len(),
        expected_jobs,
        "only control jobs reach the handler"
    );
    assert!(inputs.iter().all(|bytes| bytes == CONTROL_BYTES));
}

#[test]
fn ac1_hostile_sources_answer_corrupt_within_a_second_and_a_control_job_runs_after_each() {
    let fixture = fixture();
    let mut client = fixture.connect();
    let scratch = &fixture.scratch;
    let fifo = job_without_source(scratch, "fifo").join("source");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo")
            .success()
    );
    symlink(
        "/etc/passwd",
        job_without_source(scratch, "symlink").join("source"),
    )
    .expect("symlink");
    fs::write(scratch.join("shared"), b"shared").expect("shared file");
    fs::hard_link(
        scratch.join("shared"),
        job_without_source(scratch, "hardlink").join("source"),
    )
    .expect("hard link");
    fs::create_dir(job_without_source(scratch, "directory").join("source")).expect("directory");

    let mut expected_jobs = 0;
    for name in ["fifo", "symlink", "hardlink", "directory"] {
        send_expecting_corrupt(&mut client, render_still_in(&scratch.join(name)));
        expected_jobs += 1;

        assert_control_job_runs(&fixture, &mut client, expected_jobs);
    }
}

#[test]
fn every_job_kind_is_checked_before_the_handler_runs() {
    let fixture = fixture();
    let mut client = fixture.connect();
    let input = job_without_source(&fixture.scratch, "fifo").join("source");
    assert!(
        Command::new("mkfifo")
            .arg(&input)
            .status()
            .expect("mkfifo")
            .success()
    );
    let job_dir = fixture
        .scratch
        .join("fifo")
        .to_str()
        .expect("utf8")
        .to_owned();

    send_expecting_corrupt(
        &mut client,
        Request::Probe {
            job_dir: job_dir.clone(),
        },
    );
    send_expecting_corrupt(
        &mut client,
        Request::Transcode {
            job_dir,
            plan: media_shared::policy::Plan::Remux,
            limits: common::greedy_limits(),
        },
    );

    assert!(fixture.handler.inputs.lock().expect("inputs").is_empty());
}

#[test]
fn ac2_job_dirs_outside_the_scratch_tree_are_refused_and_nothing_outside_is_opened() {
    let fixture = fixture();
    let mut client = fixture.connect();
    let outside = TempDir::new().expect("tempdir");
    let outside_job = make_job(outside.path(), "job", b"outside bytes");
    symlink(fixture.job_dir(), fixture.scratch.join("link")).expect("symlink inside the root");
    let scratch = fixture.scratch.to_str().expect("utf8").to_owned();
    let cases = [
        "../x".to_owned(),
        format!("{scratch}/../outside"),
        outside_job.to_str().expect("utf8").to_owned(),
        "/etc".to_owned(),
        format!("{scratch}/link"),
    ];

    let mut expected_jobs = 0;
    for case in cases {
        send_expecting_corrupt(&mut client, Request::Probe { job_dir: case });
        expected_jobs += 1;

        assert_control_job_runs(&fixture, &mut client, expected_jobs);
    }

    let inputs = fixture.handler.inputs.lock().expect("inputs");
    assert!(inputs.iter().all(|bytes| bytes != b"outside bytes"));
}
