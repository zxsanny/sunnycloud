//! AZ-3874 AC-4 on one slot: the shutdown stops claims, lets a running job finish, and after the grace cancels
//! the decoder call and releases the lease.

mod api_support;
mod slot_support;

use std::time::Duration;

use media_shared::types::JobKind;
use slot_support::{Answer, Entry, Rig, ScriptedApi, loop_config, photo_probe, wait_until};
use sunnycloud_worker::{JobEnd, SlotName, SlotStep};
use tokio::time::timeout;

const WORKER: &str = "worker-shutdown";

async fn photo_rig(render_delay: Duration) -> (ScriptedApi, Rig) {
    let api = ScriptedApi::start().await;
    api.enqueue(Entry::new(1, JobKind::Stills));
    let rig = Rig::with_config(&api, SlotName::Interactive, WORKER, loop_config());
    rig.decoder.probes([Answer::Probe(photo_probe())]);
    rig.decoder.stills_fall_back_to(Answer::Image {
        width: 1440,
        height: 1080,
    });
    rig.decoder.delay(render_delay);
    (api, rig)
}

fn paths(requests: &[api_support::Recorded]) -> Vec<String> {
    requests
        .iter()
        .map(|request| format!("{} {}", request.method, request.path))
        .collect()
}

#[tokio::test]
async fn a_draining_slot_sends_no_claim() {
    // Arrange
    let (api, mut rig) = photo_rig(Duration::ZERO).await;
    rig.shutdown.drain();

    // Act
    let step = rig.slot_loop.step().await;

    // Assert
    assert_eq!(step, SlotStep::Drained);
    assert!(api.peer.requests().is_empty());
}

#[tokio::test]
async fn an_idle_slot_loop_returns_once_the_drain_starts() {
    // Arrange
    let api = ScriptedApi::start().await;
    let mut rig = Rig::new(&api, SlotName::Interactive, WORKER);
    let Rig {
        slot_loop,
        shutdown,
        ..
    } = &mut rig;

    // Act
    let ended = timeout(Duration::from_secs(10), async {
        tokio::join!(slot_loop.run(), async {
            wait_until(|| !api.peer.requests().is_empty()).await;
            shutdown.drain();
        })
    })
    .await;

    // Assert
    assert!(ended.is_ok(), "the loop kept running after the drain");
}

#[tokio::test]
async fn a_job_running_at_the_drain_finishes_and_posts_and_no_claim_follows() {
    // Arrange
    let (api, mut rig) = photo_rig(Duration::from_millis(300)).await;
    api.enqueue(Entry::new(2, JobKind::Stills));
    let Rig {
        slot_loop,
        shutdown,
        decoder,
        ..
    } = &mut rig;

    // Act
    timeout(Duration::from_secs(10), async {
        tokio::join!(slot_loop.run(), async {
            wait_until(|| decoder.requests().len() >= 2).await;
            shutdown.drain();
        })
    })
    .await
    .expect("the loop ends after the running job");
    let requests = paths(&api.peer.requests());

    // Assert
    let claims = requests
        .iter()
        .filter(|call| call.ends_with("/claims"))
        .count();
    let outputs = requests
        .iter()
        .filter(|call| call.contains("/outputs/"))
        .count();
    assert_eq!(claims, 1, "{requests:?}");
    assert_eq!(outputs, 2, "{requests:?}");
    assert_eq!(rig.jobs.jobs(JobKind::Stills, &JobEnd::Derived), 1);
    assert_eq!(rig.scratch_entries(), 0);
}

#[tokio::test]
async fn an_abort_cancels_the_decoder_call_and_releases_the_lease() {
    // Arrange
    let (api, mut rig) = photo_rig(Duration::from_secs(30)).await;
    let Rig {
        slot_loop,
        shutdown,
        decoder,
        ..
    } = &mut rig;

    // Act
    let (step, ()) = timeout(Duration::from_secs(10), async {
        tokio::join!(slot_loop.step(), async {
            wait_until(|| decoder.requests().len() >= 2).await;
            shutdown.abort();
        })
    })
    .await
    .expect("the abort ends the job");
    let requests = paths(&api.peer.requests());

    // Assert
    assert_eq!(step, SlotStep::Ran(JobEnd::Released));
    assert_eq!(decoder.cancels().len(), 1);
    let job = "/api/v1/worker-jobs/00000000-0000-0000-0000-000000000001/lease";
    assert!(
        requests.contains(&format!("DELETE {job}")),
        "no release in {requests:?}"
    );
    assert!(!requests.iter().any(|call| call.ends_with("/failure")));
    assert_eq!(rig.jobs.jobs(JobKind::Stills, &JobEnd::Released), 1);
    assert_eq!(rig.scratch_entries(), 0);
}
