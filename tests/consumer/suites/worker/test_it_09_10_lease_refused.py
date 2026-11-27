"""IT-09-10 case B (AC-79) on H2: a worker secret the API no longer accepts (403). The first 403 marks the worker not
ready, and it sends no claim and no status report until it is restarted. Its own module, so it starts from a golden
reset: case A leaves an advanced test clock and a held transcode behind. The "no token in any log" check is ST-09-1
(AZ-3879)."""
import time

import mediakit
from h2cases import HELD_TRANSCODE, PHOTO, readiness_item, run_file, wait_for_request

EVENT_SECONDS = 20
POLL_SECONDS = 0.1
QUIET_SECONDS = 3


def test_it_09_10_b_a_refused_worker_secret_stops_claims_and_status_reports_until_a_restart(stack_ctl, media, lanes):
    # Arrange
    fd, _, _, _ = run_file(media, lanes, "it-09-10-b", HELD_TRANSCODE, wanted=lambda before, after: True)
    assert wait_for_request(fd, "transcode"), fd.record()
    assert readiness_item(stack_ctl, "worker_api") == "ok"

    # Act
    stack_ctl.rotate_worker_token("drop-old")
    deadline = time.monotonic() + EVENT_SECONDS
    while readiness_item(stack_ctl, "worker_api") != "failing" and time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)
    refused = readiness_item(stack_ctl, "worker_api")
    seen_first = stack_ctl.call("db-probe", query="worker_status_seen_ms")["seen_ms"]
    queued_before = media.jobs()["queued"]
    media.commit("it-09-10-b-after.jpg", mediakit.with_marker(media.fixture(PHOTO), "it-09-10-b-after"))
    quiet = []
    deadline = time.monotonic() + QUIET_SECONDS
    while time.monotonic() < deadline:
        quiet.append(media.jobs())
        time.sleep(POLL_SECONDS * 5)
    seen_later = stack_ctl.call("db-probe", query="worker_status_seen_ms")["seen_ms"]

    # Assert
    assert refused == "failing", "the first 403 did not mark the worker not ready"
    assert seen_later == seen_first, "the supervisor kept reporting after the API refused its secret"
    assert quiet[-1]["queued"] >= queued_before + 1 and all(jobs["running"] <= 1 for jobs in quiet), quiet
