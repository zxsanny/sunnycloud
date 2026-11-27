"""IT-09-10 (AC-71, AC-79) on H2: the lease heartbeat, a lease lost to the clock (409), and a worker secret the API no
longer accepts (403; `test_it_09_10_lease_refused.py`, its own module so that it starts from a golden reset). FD holds a clip's transcode for 12 s (the case says 8 s; the margin keeps the cancel ahead of the
answer), sending `Alive` so the silence watchdog stays quiet. A transcode, not a photo render: H2's `photo_job_timeout`
is 5 s, so a held render would be cancelled by the wall watchdog first, while the video timeout is 60 s.

The heartbeat is read from the job row: every `PUT /lease` sets its expiry (`available_at`) to `now + job_lease`, so the
spacing of the expiries is the heartbeat cadence on the server's clock, free of the sampling delay. The "no token in
any log" check of case B is ST-09-1 (AZ-3879)."""
import time

from h2cases import (HELD_TRANSCODE, HOLD_MS, heartbeat_gaps, lease_samples, readiness_item, requests_of, run_file,
                     wait_for_request)
from stackkit import advance_clock

HEARTBEAT_MS = 2000
HEARTBEAT_SLACK_MS = 500
SAMPLE_SECONDS = 5
EVENT_SECONDS = 20
POLL_SECONDS = 0.1
PAST_THE_LEASE_SECONDS = 10


def test_it_09_10_a_heartbeats_every_2_s_and_a_lease_lost_to_the_clock_cancels_the_job_without_a_post(stack_ctl, media, lanes):
    # Arrange
    derived_before = stack_ctl.call("db-probe", query="derived_rows")["transcode"]
    fd, _, before, _ = run_file(media, lanes, "it-09-10-a", HELD_TRANSCODE, wanted=lambda before, after: True)
    assert wait_for_request(fd, "transcode"), fd.record()
    started = stack_ctl.call("inspect", service="worker")["started_at"]

    # Act
    samples = lease_samples(stack_ctl, SAMPLE_SECONDS)
    gaps = heartbeat_gaps(samples)
    advance_clock(stack_ctl, PAST_THE_LEASE_SECONDS)
    deadline = time.monotonic() + EVENT_SECONDS
    while not requests_of(fd.record(), "cancel") and time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)
    record = fd.record()
    derived = stack_ctl.call("db-probe", query="derived_rows")
    after = media.jobs()
    worker_api = readiness_item(stack_ctl, "worker_api")

    # Assert
    assert gaps and all(abs(gap - HEARTBEAT_MS) <= HEARTBEAT_SLACK_MS for gap in gaps), (gaps, samples)
    transcode, cancels = requests_of(record, "transcode")[0], requests_of(record, "cancel")
    assert cancels and cancels[0]["at_ms"] - transcode["at_ms"] < HOLD_MS, record
    assert derived["transcode"] == derived_before, "the attempt that lost its lease posted its output"
    assert after["dead"] == before["dead"]
    # It keeps polling with the same id: the job, requeued, is claimed again on the same connection.
    retried = [line for line in requests_of(fd.record(), "transcode")
               if line["at_ms"] > cancels[0]["at_ms"] and line["job"] == transcode["job"]]
    assert retried and retried[0]["conn"] == transcode["conn"], fd.record()
    assert worker_api == "ok"
    assert stack_ctl.call("inspect", service="worker")["started_at"] == started
