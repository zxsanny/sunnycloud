"""IT-09-8 (AC-71) on H2: a photo job whose revision is gone before the fetch is closed as obsolete, with no decoder
call and no retry, and its scratch is removed.

The supervisor is gated out (FD fails `self_check` on both lanes) while the file is deleted and the test clock moves
past retention; then FD passes the gate and the supervisor claims the job."""
import time

import mediakit
from fdkit import FAILED_SELF_CHECK, JOB_REQUESTS
from stackkit import advance_clock

GATE_SECONDS = 30
JOBS_SECONDS = 60
POLL_SECONDS = 0.25
# Past the stack's 30 s retention and 60 s tombstone horizon (compose.test.yml), so the revision is purged.
PAST_RETENTION_SECONDS = 120


def revisions(ctl):
    return ctl.call("db-probe", query="table_rows")["revisions"]


def obsolete_count(ctl):
    samples = ctl.worker_metrics()
    return sum(value for name, value in samples.items()
               if name.startswith("sunnycloud_jobs_total{") and 'result="obsolete"' in name)


def test_it_09_8_a_revision_gone_before_the_fetch_closes_the_job_as_obsolete(stack_ctl, media, lanes):
    # Arrange
    for lane in ("interactive", "general"):
        lanes(lane, {"self_check": FAILED_SELF_CHECK}, restart_worker=lane == "general")
    assert stack_ctl.wait_worker_readyz(503, GATE_SECONDS) == 503
    before = media.jobs()
    committed = media.commit("it-09-8.jpg", mediakit.with_marker(media.fixture("photo-orientation6.jpg"), "it-09-8"))
    kept = revisions(stack_ctl)
    media.drive.delete(committed["node_id"])
    advance_clock(stack_ctl, PAST_RETENTION_SECONDS)
    deadline = time.monotonic() + JOBS_SECONDS
    while revisions(stack_ctl) >= kept and time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)
    purged = revisions(stack_ctl) < kept

    # Act: FD passes the gate now, and the supervisor claims the waiting job.
    fds = [lanes("interactive", {}, restart_worker=False), lanes("general", {})]
    deadline = time.monotonic() + JOBS_SECONDS
    while True:
        after = media.jobs()
        if after["obsolete"] > before["obsolete"] or time.monotonic() > deadline:
            break
        time.sleep(POLL_SECONDS)
    attempts = stack_ctl.call("db-probe", query="job_attempts")
    scratch = {lane: stack_ctl.call("inspect", scratch=lane)["entries"] for lane in ("interactive", "general")}

    # Assert
    assert purged, "the deleted file's revision was not purged after the clock passed retention"
    assert after["obsolete"] - before["obsolete"] == 1, (before, after)
    assert [line for fd in fds for line in fd.record() if line["kind"] in JOB_REQUESTS] == []
    assert attempts["most"] <= 1, attempts
    assert scratch == {"interactive": [], "general": []}
    assert obsolete_count(stack_ctl) == 1.0
