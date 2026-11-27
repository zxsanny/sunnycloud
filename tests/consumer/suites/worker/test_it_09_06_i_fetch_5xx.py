"""IT-09-6 (i) (AC-45) on H2: an input fetch the server answers with 5xx fails the job `Retry`, and the job is queued
again with its backoff (`job_retry_backoff` 1 s on the stack, doubled per attempt). The 5xx comes from the real API
(decision D2a): one bit of the input blob's first chunk is flipped while the supervisor takes no job, so the blob reader
refuses that chunk before it sends a body byte; every later attempt reads the same corrupt chunk.

A module of its own: the corrupt read marks the blob unavailable and turns the blob health degraded until the module's
golden restore. The test clock is not moved."""
import time

import mediakit
from fdkit import job_requests
from h2cases import CLEAN_PHOTO, JOBS_SECONDS, PHOTO, POLL_SECONDS, gate_out_both_lanes, input_errors, job_rows, \
    wait_for_row

RETRY_BACKOFF_SECONDS = 1.0


def wait_for_errors(ctl, after, count):
    """The input route's error answers logged after `after`, once there are `count` of them (or what came)."""
    deadline = time.monotonic() + JOBS_SECONDS
    while len(input_errors(ctl, after)) < count and time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)
    return input_errors(ctl, after)


def test_it_09_6_i_a_fetch_the_server_answers_with_5xx_fails_retry_and_the_job_is_queued_again(stack_ctl, media, lanes):
    # Arrange: the job waits queued (both lanes gated out) while the harness flips a bit of its blob.
    gate_out_both_lanes(stack_ctl, lanes)
    before = job_rows(stack_ctl)
    media.commit("it-09-6-i.jpg", mediakit.with_marker(media.fixture(PHOTO), "it-09-6-i"))
    job, row = wait_for_row(stack_ctl, lambda job, row: job not in before and row["blob"])
    assert job, "the committed photo got no job"
    flip = stack_ctl.call("bit-flip", blob_id=row["blob"])
    mark = max((at for at, _ in input_errors(stack_ctl)), default=0.0)

    # Act: FD passes the general gate now, and the supervisor claims the job; its retry reads the same chunk.
    fd = lanes("general", CLEAN_PHOTO)
    _, failed = wait_for_row(stack_ctl, lambda seen, row: seen == job and row["last_error"])
    errors = wait_for_errors(stack_ctl, mark, 2)

    # Assert
    assert flip["after"] == flip["before"] ^ 1, flip
    assert len(errors) >= 2 and all(500 <= status < 600 for _, status in errors), errors
    assert (failed["last_error"], failed["status"] in ("queued", "running")) == ("Transient", True), failed
    assert errors[1][0] - errors[0][0] >= RETRY_BACKOFF_SECONDS, errors
    assert job_requests(fd) == []
