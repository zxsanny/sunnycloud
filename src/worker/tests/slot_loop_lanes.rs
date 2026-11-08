//! AZ-3872 AC-1: the reserved interactive slot, the rotating general slot, one job at a time.

mod api_support;
mod slot_support;

use media_shared::types::JobKind;
use slot_support::{Answer, Entry, Rig, ScriptedApi, compliant_probe, photo_probe};
use std::time::Duration;
use sunnycloud_worker::{SlotLoop, SlotName, SlotStep};

const INTERACTIVE: &str = "worker-interactive";
const GENERAL: &str = "worker-general";
const JOBS_PER_LANE: u128 = 20;
const RENDER_TIME: Duration = Duration::from_millis(20);

async fn drain(slot_loop: &mut SlotLoop) -> usize {
    let mut ran = 0;
    loop {
        match slot_loop.step().await {
            SlotStep::Ran(_) => ran += 1,
            SlotStep::Idle | SlotStep::Stopped | SlotStep::Drained => return ran,
        }
    }
}

fn script(rig: &Rig, probe: Answer) {
    rig.decoder.probes_fall_back_to(probe);
    rig.decoder.stills_fall_back_to(Answer::Image {
        width: 1440,
        height: 1080,
    });
    rig.decoder.delay(RENDER_TIME);
}

#[tokio::test]
async fn interactive_slot_claims_images_only_and_the_general_slot_alternates_lanes() {
    // Arrange
    let api = ScriptedApi::start().await;
    for n in 0..JOBS_PER_LANE {
        api.enqueue(Entry::new(n, JobKind::Stills));
        api.enqueue(Entry::new(100 + n, JobKind::Poster));
    }
    let mut interactive = Rig::new(&api, SlotName::Interactive, INTERACTIVE);
    let mut general = Rig::new(&api, SlotName::General, GENERAL);
    script(&interactive, Answer::Probe(photo_probe()));
    script(&general, Answer::Probe(compliant_probe(9_000)));

    // Act
    let (ran_interactive, ran_general) = tokio::join!(
        drain(&mut interactive.slot_loop),
        drain(&mut general.slot_loop)
    );

    // Assert
    let requests = api.peer.requests();
    let lanes = |worker: &str| -> Vec<String> {
        requests
            .iter()
            .filter(|request| request.header("x-worker-id") == Some(worker))
            .filter(|request| request.path.ends_with("/claims"))
            .map(|request| {
                slot_support::json(request)["lane"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    };
    let interactive_lanes = lanes(INTERACTIVE);
    let general_lanes = lanes(GENERAL);
    assert!(interactive_lanes.iter().all(|lane| lane == "image"));
    let first_ten = &general_lanes[..10];
    assert!(
        first_ten
            .iter()
            .enumerate()
            .all(|(index, lane)| lane == if index % 2 == 0 { "video" } else { "image" }),
        "{first_ten:?}"
    );
    assert_eq!(ran_interactive + ran_general, 2 * JOBS_PER_LANE as usize);
    assert_eq!(interactive.decoder.most_in_flight(), 1);
    assert_eq!(general.decoder.most_in_flight(), 1);
    assert_eq!(interactive.scratch_entries() + general.scratch_entries(), 0);
}
