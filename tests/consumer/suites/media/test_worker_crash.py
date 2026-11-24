"""AZ-3661 AC-71 (risk R4) on the stack: a supervisor killed with SIGKILL while a job is in the decoder leaves exactly one
result after the retry. One run for a photo job and one for a video job.

"During decoding" is made deterministic: the decoder container is frozen (`pause`), so the job the supervisor has handed
over stays in the decoder; the supervisor is then killed, the decoder thawed, the supervisor started again and the test
clock moved past the job lease (10 s in this stack), so the reaper gives the job to the new supervisor. The legs "during
the input fetch" and "during the output post" need a hold in the supervisor that the stack does not have (each phase
lasts milliseconds): they are NOT-RUN below.
"""
import time

import pytest

import mediakit
from e2e_report import skip_not_run
from stackkit import advance_clock

LEASE_PASSED_SECONDS = 30
RUNNING_SECONDS = 30
RETRY_SECONDS = 90
JANITOR_SECONDS = 30
SCRATCH_SECONDS = 15


def wait_for(read, done, seconds):
    deadline = time.monotonic() + seconds
    while True:
        seen = read()
        if done(seen) or time.monotonic() > deadline:
            return seen
        time.sleep(1.0)


DECODERS = ("decoder-interactive", "decoder-general")


def kill_in_the_decoder(ctl, media, name, data):
    """Commits `data`, waits until a job is running in a (frozen) decoder, kills the supervisor, thaws the decoders. Both
    decoders are frozen: a photo may be claimed by either slot, a clip's two jobs by both."""
    for decoder in DECODERS:
        ctl.call("process-kill", service=decoder, op="pause")
    try:
        file = media.commit(name, data)
        running = wait_for(media.jobs, lambda jobs: jobs["running"] >= 1, RUNNING_SECONDS)
        assert running["running"] >= 1, f"the supervisor never claimed the job: {running}"
        ctl.call("process-kill", service="worker", op="kill")
    finally:
        for decoder in DECODERS:
            ctl.call("process-kill", service=decoder, op="unpause")
    return file


def restart_and_retry(ctl, media, closed):
    ctl.call("process-kill", service="worker", op="start")
    advance_clock(ctl, LEASE_PASSED_SECONDS)
    return wait_for(media.jobs, lambda jobs: jobs["queued"] == 0 and jobs["running"] == 0 and jobs["derived"] >= closed, RETRY_SECONDS)


def assert_one_result_and_clean(ctl, media, derived):
    assert ctl.call("db-probe", query="job_attempts") == {"most": 2}, "a job was claimed twice: the killed attempt and its retry"
    assert media.derived() == derived, "exactly one derived row per kind: no second row from the killed attempt"
    refs = wait_for(lambda: ctl.call("db-probe", query="blob_refs"), lambda r: r["unreferenced"] == 0, JANITOR_SECONDS)
    assert refs["unreferenced"] == 0, f"a derived blob nothing refers to is left after the derived janitor: {refs}"
    for lane in ("interactive", "general"):
        left = wait_for(lambda: ctl.call("inspect", scratch=lane)["entries"], lambda e: e == [], SCRATCH_SECONDS)
        assert left == [], f"scratch of {lane} is not empty: {left}"


@pytest.fixture(autouse=True)
def stack_whole_again(stack_ctl):
    """Whatever a test breaks (a frozen decoder, a dead supervisor), the next one starts with both up."""
    yield
    for service in DECODERS:
        if stack_ctl.call("inspect", service=service)["status"] == "paused":
            stack_ctl.call("process-kill", service=service, op="unpause")
    if stack_ctl.call("inspect", service="worker")["status"] != "running":
        stack_ctl.call("process-kill", service="worker", op="start")


def test_supervisor_killed_while_a_photo_is_decoded_leaves_one_thumbnail_and_one_preview(stack_ctl, media):
    # Arrange
    photo = mediakit.with_marker(media.fixture("photo-orientation6.jpg"), "crash-photo")
    before = media.derived()

    # Act
    file = kill_in_the_decoder(stack_ctl, media, "crash-photo.jpg", photo)
    retried = restart_and_retry(stack_ctl, media, closed=media.jobs()["derived"] + 1)

    # Assert
    assert retried["queued"] == 0 and retried["running"] == 0, retried
    assert media.wait_state(file["content_hash"], "ready") == "ready"
    assert_one_result_and_clean(stack_ctl, media, {"thumb": before["thumb"] + 1, "preview": before["preview"] + 1,
                                                   "transcode": before["transcode"], "total": before["total"] + 2})


def test_supervisor_killed_while_a_clip_is_decoded_leaves_one_poster_and_one_transcode(stack_ctl, media):
    # Arrange
    clip = mediakit.with_marker(media.fixture("clip-noncompliant.mp4"), "crash-clip")
    before = media.derived()

    # Act
    file = kill_in_the_decoder(stack_ctl, media, "crash-clip.mp4", clip)
    retried = restart_and_retry(stack_ctl, media, closed=media.jobs()["derived"] + 2)

    # Assert
    assert retried["queued"] == 0 and retried["running"] == 0, retried
    assert media.wait_state(file["content_hash"], "ready") == "ready"
    assert media.derived_file(file["node_id"], "transcode")[0] == 200
    assert_one_result_and_clean(stack_ctl, media, {"thumb": before["thumb"] + 1, "preview": before["preview"],
                                                   "transcode": before["transcode"] + 1, "total": before["total"] + 2})


def test_supervisor_killed_during_the_input_fetch_leaves_one_result():
    skip_not_run("NO-SERVICE", "the fetch of a small input lasts milliseconds and the stack has no hold on it; needs a slow "
                               "listener or a large input on the worker listener")


def test_supervisor_killed_during_the_output_post_leaves_one_result():
    skip_not_run("NO-SERVICE", "the post lasts milliseconds and the stack has no hold on it; needs a hold between decoder "
                               "output and post in a test build of the supervisor")
