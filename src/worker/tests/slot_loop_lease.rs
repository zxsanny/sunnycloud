//! AZ-3873 AC-1 and AC-2: the lease heartbeat of a running job, and the quiet abort when the lease is lost.
//! The job lease is 300 ms here, so the heartbeat period is 100 ms.

mod api_support;
mod slot_support;

use std::time::{Duration, Instant};

use media_shared::types::JobKind;
use slot_support::{Answer, Entry, Rig, ScriptedApi, calls, loop_config, photo_probe, wait_until};
use sunnycloud_worker::{JobEnd, SlotName, SlotStep};

const WORKER: &str = "worker-lease";
const LEASE_MS: u64 = 300;
const PERIOD: Duration = Duration::from_millis(LEASE_MS / 3);

async fn photo_rig(entry: Entry, render_delay: Duration) -> (ScriptedApi, Rig) {
    let api = ScriptedApi::start().await;
    api.enqueue(entry);
    let rig = Rig::with_config(&api, SlotName::Interactive, WORKER, loop_config());
    rig.decoder.probes([Answer::Probe(photo_probe())]);
    rig.decoder.stills_fall_back_to(Answer::Image {
        width: 1440,
        height: 1080,
    });
    rig.decoder.delay(render_delay);
    (api, rig)
}

fn lease_times(requests: &[api_support::Recorded]) -> Vec<Instant> {
    requests
        .iter()
        .filter(|request| request.path.ends_with("/lease"))
        .map(|request| request.at)
        .collect()
}

#[tokio::test]
async fn heartbeats_arrive_at_a_third_of_the_lease_while_the_job_runs_and_stop_with_it() {
    // Arrange
    let entry = Entry::new(1, JobKind::Stills).lease_ms(LEASE_MS);
    let (api, mut rig) = photo_rig(entry, Duration::from_millis(450)).await;

    // Act
    let end = rig.run_one().await;
    let during = api.peer.requests();
    tokio::time::sleep(PERIOD * 4).await;
    let after = api.peer.requests();

    // Assert
    let times = lease_times(&during);
    let gaps: Vec<Duration> = times.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert_eq!(end, JobEnd::Derived);
    assert!(times.len() >= 4, "heartbeats: {}", times.len());
    assert!(
        gaps.iter()
            .all(|gap| gap.abs_diff(PERIOD) <= Duration::from_millis(60)),
        "gaps: {gaps:?}"
    );
    assert!(
        lease_times(&after).is_empty(),
        "a heartbeat after the job ended"
    );
}

#[tokio::test]
async fn a_lost_lease_cancels_the_decoder_and_sends_nothing_more_for_the_job() {
    // Arrange
    let entry = Entry::new(1, JobKind::Stills).lease_ms(LEASE_MS);
    let job = entry.id();
    let (api, mut rig) = photo_rig(entry, Duration::from_secs(30)).await;

    // Act
    let (step, expired_at) = tokio::join!(rig.slot_loop.step(), async {
        wait_until(|| rig.decoder.requests().len() == 2).await;
        api.expire_lease(job);
        Instant::now()
    });
    let cancels = rig.decoder.cancels();
    let next = rig.slot_loop.step().await;
    let requests = api.peer.requests();

    // Assert
    let heartbeat_free: Vec<String> = calls(&requests)
        .into_iter()
        .filter(|call| call != "lease")
        .collect();
    assert_eq!(step, SlotStep::Ran(JobEnd::LeaseLost));
    assert_eq!(cancels.len(), 1);
    assert!(cancels[0] - expired_at < Duration::from_secs(1));
    assert_eq!(heartbeat_free, ["claim image", "input", "claim image"]);
    assert_eq!(rig.scratch_entries(), 0);
    assert_eq!(next, SlotStep::Idle);
    assert_eq!(
        requests.last().and_then(|r| r.header("x-worker-id")),
        Some(WORKER)
    );
    assert_eq!(
        rig.decoder.hellos(),
        1,
        "the answered cancel keeps the connection"
    );
}

#[tokio::test]
async fn a_lease_refused_with_403_ends_the_job_quietly_and_stops_the_worker() {
    // Arrange
    let entry = Entry::new(1, JobKind::Stills)
        .lease_ms(LEASE_MS)
        .lease_status(403);
    let (api, mut rig) = photo_rig(entry, Duration::from_secs(30)).await;

    // Act
    let step = rig.slot_loop.step().await;
    let next = rig.slot_loop.step().await;

    // Assert
    assert_eq!(step, SlotStep::Ran(JobEnd::Refused));
    assert_eq!(next, SlotStep::Stopped);
    assert!(rig.access.is_refused());
    let heartbeat_free: Vec<String> = calls(&api.peer.requests())
        .into_iter()
        .filter(|call| call != "lease")
        .collect();
    assert_eq!(heartbeat_free, ["claim image", "input"]);
}
