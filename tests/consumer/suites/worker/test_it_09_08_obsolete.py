"""IT-09-8 (AC-71) on H2: a photo job whose revision is gone before the fetch is closed as obsolete, with no decoder
call and no retry, and its scratch is removed.

The supervisor is gated out (FD fails `self_check` on both lanes) while the file is deleted and the test clock moves
past retention; then FD passes the gate and the supervisor claims the job."""
import time

import mediakit
from fdkit import JOB_REQUESTS
from h2cases import gate_out_both_lanes, job_rows, new_rows, worker_jobs_total
from stackkit import advance_clock

JOBS_SECONDS = 60
POLL_SECONDS = 0.25
# Past the stack's 30 s retention and 60 s tombstone horizon (compose.test.yml), so the revision is purged.
PAST_RETENTION_SECONDS = 120


def revisions(ctl):
    return ctl.call("db-probe", query="table_rows")["revisions"]


def obsolete_count(ctl):
    return worker_jobs_total(ctl, "stills", "obsolete")


def test_it_09_8_a_revision_gone_before_the_fetch_closes_the_job_as_obsolete(stack_ctl, media, lanes):
    # Arrange
    gate_out_both_lanes(stack_ctl, lanes)
    before, rows_before = media.jobs(), job_rows(stack_ctl)
    committed = media.commit("it-09-8.jpg", mediakit.with_marker(media.fixture("photo-orientation6.jpg"), "it-09-8"))
    kept = revisions(stack_ctl)
    media.drive.delete(committed["node_id"])
    advance_clock(stack_ctl, PAST_RETENTION_SECONDS)
    deadline = time.monotonic() + JOBS_SECONDS
    while revisions(stack_ctl) >= kept and time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)
    purged = revisions(stack_ctl) < kept

    # Act: FD passes the gate now, and a new supervisor claims the waiting job. The old one is stopped first: one that
    # reconnected to a passing FD before its restart would close the job and take its `jobs_total` sample with it.
    stack_ctl.call("process-kill", service="worker", op="kill")
    fds = [lanes("interactive", {}, restart_worker=False), lanes("general", {}, restart_worker=False)]
    stack_ctl.call("process-kill", service="worker", op="start")
    deadline = time.monotonic() + JOBS_SECONDS
    while True:
        after = media.jobs()
        if after["obsolete"] > before["obsolete"] or time.monotonic() > deadline:
            break
        time.sleep(POLL_SECONDS)
    rows = list(new_rows(stack_ctl, rows_before).values())
    scratch = {lane: stack_ctl.call("inspect", scratch=lane)["entries"] for lane in ("interactive", "general")}

    # Assert
    assert purged, "the deleted file's revision was not purged after the clock passed retention"
    assert after["obsolete"] - before["obsolete"] == 1, (before, after)
    assert [line for fd in fds for line in fd.record() if line["kind"] in JOB_REQUESTS] == []
    assert [row["attempts"] for row in rows] == [1], rows
    assert scratch == {"interactive": [], "general": []}
    assert obsolete_count(stack_ctl) == 1.0
