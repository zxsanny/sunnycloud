//! IT-09-15 in the slot loop: a runner that goes away while the slot is idle is noticed at the start of the next
//! round, before any claim, and the slot reconnects and repeats `hello` and `self_check` first.

mod api_support;
mod decoder_support;
mod slot_support;

use std::io::ErrorKind;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use decoder_support::eventually;
use media_shared::types::JobKind;
use slot_support::{Answer, Entry, Rig, ScriptedApi, calls, photo_probe};
use sunnycloud_worker::{JobEnd, SlotName};

const WORKER: &str = "worker-reconnect";
const WAIT: Duration = Duration::from_secs(10);
const OUTAGE: Duration = Duration::from_millis(200);

fn refused(rig: &Rig) -> bool {
    matches!(UnixStream::connect(&rig.socket), Err(error) if error.kind() == ErrorKind::ConnectionRefused)
}

#[tokio::test]
async fn a_runner_that_goes_away_while_the_slot_is_idle_is_gated_again_before_the_next_claim() {
    // Arrange
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Stills));
    api.enqueue(Entry::new(2, JobKind::Stills));
    let mut rig = Rig::new(&api, SlotName::Interactive, WORKER);
    rig.decoder
        .probes([Answer::Probe(photo_probe()), Answer::Probe(photo_probe())]);
    rig.decoder.stills_fall_back_to(Answer::image(1440, 1080));
    let first = rig.run_one().await;
    let first_calls = calls(&api.peer.requests());

    // Act
    rig.decoder.outage(OUTAGE);
    assert!(
        eventually(WAIT, || refused(&rig)).await,
        "the runner's socket never refused"
    );
    let second = rig.run_one().await;

    // Assert
    assert_eq!((first, second), (JobEnd::Derived, JobEnd::Derived));
    assert_eq!(rig.decoder.hellos(), 2);
    assert_eq!(rig.decoder_metrics.restarts(SlotName::Interactive), 1);
    let one_job = ["claim image", "input", "output preview", "output thumb"];
    assert_eq!(first_calls, one_job);
    assert_eq!(
        calls(&api.peer.requests()),
        one_job,
        "one claim, on the new connection, and no failure"
    );
}
