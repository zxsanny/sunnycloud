//! AZ-3857 and AZ-3854 against hostile tool behaviour: the real runner binary is given a
//! `PATH` whose `vipsthumbnail` is a stub that hangs, crashes or writes a bad output. Only the
//! hostile behaviour is stubbed; `vipsheader` is the real one and every outcome is the real
//! runner's own.
mod common;

use std::time::{Duration, Instant};

use common::media::{Media, MediaRunner, limits, path_with_stub, render, wait_until_gone};
use media_shared::decoder_protocol::{DecodeError, LimitKind, Response};

fn photo() -> Vec<u8> {
    Media::new().image("photo.jpg", 400, 300)
}

#[test]
fn a_hung_tool_is_killed_at_the_wall_limit_and_nothing_of_it_stays_alive() {
    let (_bin, path) = path_with_stub("sleep 59.111 & exec sleep 59.222");
    let mut runner = MediaRunner::start_with(&[], &path);
    let mut client = runner.connect();
    let job = runner.job(&photo());
    let mut short = limits();
    short.wall = 700;
    let started = Instant::now();

    let response = render(&mut client, &job, 320, short);

    assert_eq!(
        response,
        Response::Error(DecodeError::LimitExceeded {
            limit: LimitKind::Wall
        })
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    wait_until_gone(&["59.111", "59.222"]);
    assert!(!job.join("work").exists());
    assert_eq!(runner.exit_code_within(Duration::from_secs(5)), Some(3));
}

#[test]
fn a_tool_that_crashes_on_the_input_is_a_typed_corrupt_answer() {
    let (_bin, path) = path_with_stub("kill -SEGV $$");
    let mut runner = MediaRunner::start_with(&[], &path);
    let mut client = runner.connect();
    let job = runner.job(&photo());

    let response = render(&mut client, &job, 320, limits());

    assert_eq!(response, Response::Error(DecodeError::Corrupt));
    assert_eq!(runner.exit_code_within(Duration::from_secs(5)), Some(3));
}

#[test]
fn a_tool_that_exits_zero_without_an_output_or_with_a_fake_one_is_an_invalid_output() {
    let stubs = [
        ("no output", "exit 0"),
        ("not a webp", "printf 'GIF89a not webp' > result.webp"),
        ("empty", ": > result.webp"),
        ("a link to a secret", "ln -s /etc/passwd result.webp"),
    ];

    for (name, stub) in stubs {
        let (_bin, path) = path_with_stub(stub);
        let mut runner = MediaRunner::start_with(&[], &path);
        let mut client = runner.connect();
        let job = runner.job(&photo());

        let response = render(&mut client, &job, 320, limits());

        assert_eq!(
            response,
            Response::Error(DecodeError::OutputInvalid),
            "{name}"
        );
    }
}
