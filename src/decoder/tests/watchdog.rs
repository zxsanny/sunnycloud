//! AC-5 of AZ-3856: a long job does not trip the watchdog. The runner is the library loop with
//! a real watchdog thread; an abort would end this test process, so passing proves it did not.
mod common;

use std::time::Duration;

use common::{Fixture, Received, Scripted};
use media_shared::decoder_protocol::{Event, ImageResult, Reply, Response};

#[test]
fn ac5_a_30_second_job_with_a_3_second_watchdog_runs_to_the_end() {
    let fixture = Fixture::start_with(
        Scripted::new(Duration::from_secs(30)).lines(30),
        Duration::from_millis(500),
        |config| config.watchdog = Duration::from_secs(3),
    );
    let mut client = fixture.connect();

    client.send(fixture.render_still());
    let mut alive = 0;
    let response = loop {
        match client.recv(Duration::from_secs(5)) {
            Received::Reply(Reply::Event(Event::Alive)) => alive += 1,
            Received::Reply(Reply::Event(Event::Progress { .. })) => {}
            Received::Reply(Reply::Response(response)) => break response,
            other => panic!("unexpected {other:?}"),
        }
    };

    assert_eq!(
        response,
        Response::ImageResult(ImageResult {
            width: 1,
            height: 1,
            bytes: 1
        })
    );
    assert!(
        alive >= 50,
        "the loop stayed alive: {alive} Alive events in 30 s"
    );
}
