"""IT-09-9 (AC-27) on H2: two slots, two lanes. With 20 poster and transcode jobs and 20 stills jobs queued and FD on
both sockets answering after 300 ms, the interactive slot takes only image-lane jobs, the general slot takes both
lanes and both progress, and each slot runs one job at a time.

Not asserted here: "a priority-0 stills job queued behind bulk is claimed by the next free image-capable slot" needs
more queued jobs than `bulk_queue_threshold` (200) to make any job bulk; that rule is 08's claim order
(`components/08_media_jobs/tests/media/queue_claim.rs`)."""
import time

import mediakit
from fdkit import job_requests
from h2cases import CLIP, PHOTO, gate_out_both_lanes, postable_image, postable_transcode, terminal, video_probe

CLIPS = 10
PHOTOS = 20
DRAIN_SECONDS = 180
POLL_SECONDS = 0.5
SLOW = {"probe": {"default": {"over_time": {"delay_ms": 300, "answer": {"probe": video_probe()}}}},
        "render_still": {"default": postable_image()}, "transcode": {"default": postable_transcode()}}


def job_order(fd):
    """The jobs a lane's FD served, in the order their first request arrived."""
    order = []
    for line in job_requests(fd):
        if line["job"] not in order:
            order.append(line["job"])
    return order


def interleaved(fd):
    """True when a job's requests resume after another job's: two jobs at once on one slot."""
    seen, current = set(), None
    for line in job_requests(fd):
        if line["job"] != current:
            if line["job"] in seen:
                return True
            seen.add(line["job"])
            current = line["job"]
    return False


def test_it_09_9_the_interactive_slot_takes_images_only_and_the_general_slot_serves_both_lanes(stack_ctl, media, lanes):
    # Arrange: the slots are gated out while the queue fills, then both FD pass the gate.
    gate_out_both_lanes(stack_ctl, lanes)
    before = media.jobs()
    for n in range(CLIPS):
        media.commit(f"it-09-9-clip-{n}.mp4", mediakit.with_marker(media.fixture(CLIP), f"it-09-9-clip-{n}"))
    for n in range(PHOTOS):
        media.commit(f"it-09-9-photo-{n}.jpg", mediakit.with_marker(media.fixture(PHOTO), f"it-09-9-photo-{n}"))
    queued = media.jobs()["queued"] - before["queued"]

    # Act: the supervisor is stopped while both lanes are swapped and starts once, so no slot begins a job that a
    # later restart would cut (a job cut by a kill runs again after its lease, which would look like two at once).
    stack_ctl.call("process-kill", service="worker", op="kill")
    interactive = lanes("interactive", SLOW, restart_worker=False)
    general = lanes("general", SLOW, restart_worker=False)
    stack_ctl.call("process-kill", service="worker", op="start")
    deadline = time.monotonic() + DRAIN_SECONDS
    while True:
        jobs = media.jobs()
        if (jobs["queued"] == 0 and jobs["running"] == 0) or time.monotonic() > deadline:
            break
        time.sleep(POLL_SECONDS)
    lane_of = dict(value.split(":", 1) for value in stack_ctl.call("db-probe", query="job_lanes")["values"])
    interactive_lanes = [lane_of[job] for job in job_order(interactive)]
    general_lanes = [lane_of[job] for job in job_order(general)]

    # Assert
    assert queued == CLIPS * 2 + PHOTOS, queued
    assert terminal(jobs) - terminal(before) == queued, (before, jobs)
    assert interactive_lanes and set(interactive_lanes) == {"image"}, interactive_lanes
    first_ten = general_lanes[:10]
    assert first_ten.count("video") >= 4 and first_ten.count("image") >= 4, general_lanes
    assert len(interactive_lanes) + len(general_lanes) == queued
    assert not interleaved(interactive) and not interleaved(general), "a slot ran two jobs at once"
