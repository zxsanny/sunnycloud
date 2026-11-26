"""IT-09-1 (AC-25) on H2: the supervisor sweeps both scratch roots at its start, takes jobs only once each decoder passed
`hello` and an all-ok `self_check`, and carries no database access, blob mount or media tool.

FD answers the gate in the gate cases; the jobs they let through are protocol traffic only (FD's default answers), no
media outcome is claimed."""
import time

import mediakit
from fdkit import FAILED_SELF_CHECK, job_requests

GATE_SECONDS = 30
READY_SECONDS = 90
# The window in which a gated-out supervisor must not claim: 30 claim polls at the H2 interval of 100 ms.
QUIET_SECONDS = 3
POLL_SECONDS = 0.25
LANES = ("interactive", "general")


def selfcheck_gauges(ctl):
    samples = ctl.worker_metrics()
    return [samples.get(f'sunnycloud_worker_selfcheck_ok{{slot="{lane}"}}') for lane in LANES]


def wait_latest_decoder_ok(ctl, wanted, seconds=GATE_SECONDS):
    """`decoder_ok` of the latest status report (1, 0, or -1 before any) once it is `wanted`, or the last one read: a
    restarted supervisor reports under a new id, and the earlier rows stay."""
    deadline = time.monotonic() + seconds
    while True:
        latest = ctl.call("db-probe", query="worker_status_latest")["decoder_ok"]
        if latest == wanted or time.monotonic() > deadline:
            return latest
        time.sleep(POLL_SECONDS)


def photo(media, marker):
    return media.commit(f"{marker}.jpg", mediakit.with_marker(media.fixture("photo-orientation6.jpg"), marker))


def test_it_09_1_leftovers_in_both_scratch_roots_are_swept_at_start(stack_ctl):
    # Arrange
    for lane in LANES:
        stack_ctl.scratch_fill(lane, trees=2)
    filled = {lane: sorted(stack_ctl.call("inspect", scratch=lane)["entries"]) for lane in LANES}

    # Act
    stack_ctl.restart_worker()
    ready = stack_ctl.wait_worker_readyz(200, READY_SECONDS)
    left = {lane: stack_ctl.call("inspect", scratch=lane)["entries"] for lane in LANES}

    # Assert
    assert filled == {lane: ["leftover-0", "leftover-1", "stray.tmp"] for lane in LANES}
    assert ready == 200
    assert left == {lane: [] for lane in LANES}


def test_it_09_1_with_every_item_ok_both_slots_are_ready_and_take_a_job(stack_ctl, media, lanes):
    # Arrange
    fds = [lanes("interactive", {}, restart_worker=False), lanes("general", {})]

    # Act
    ready = stack_ctl.wait_worker_readyz(200, GATE_SECONDS)
    gauges = selfcheck_gauges(stack_ctl)
    photo(media, "it-09-1-gate-ok")
    deadline = time.monotonic() + GATE_SECONDS
    while not any(job_requests(fd) for fd in fds) and time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)

    # Assert
    assert ready == 200
    assert gauges == [1.0, 1.0]
    assert any(job_requests(fd) for fd in fds), [fd.record() for fd in fds]


def test_it_09_1_a_failed_self_check_item_keeps_the_supervisor_from_claiming(stack_ctl, media, lanes):
    # Arrange
    fds = [lanes("interactive", {"self_check": FAILED_SELF_CHECK}, restart_worker=False),
           lanes("general", {"self_check": FAILED_SELF_CHECK})]
    for fd in fds:
        fd.wait_for(["hello", "self_check"], GATE_SECONDS)

    # Act
    not_ready = stack_ctl.wait_worker_readyz(503, GATE_SECONDS)
    gauges = selfcheck_gauges(stack_ctl)
    reported = wait_latest_decoder_ok(stack_ctl, 0)
    photo(media, "it-09-1-gate-failed")
    seen = []
    deadline = time.monotonic() + QUIET_SECONDS
    while time.monotonic() < deadline:
        seen.append(media.jobs())
        time.sleep(POLL_SECONDS)

    # Assert
    assert not_ready == 503
    assert gauges == [0.0, 0.0]
    assert reported == 0, "the latest status report does not carry decoder_ok = false"
    assert seen and all(jobs["running"] == 0 for jobs in seen), seen
    assert seen[-1]["queued"] >= 1, seen
    assert [job_requests(fd) for fd in fds] == [[], []]


def test_it_09_1_the_worker_container_has_no_database_access_blob_mount_or_media_tool(stack_ctl):
    # Act
    facts = stack_ctl.container_facts()

    # Assert
    database = [name for name in facts["env_names"]
                if "DB" in name.upper().split("_") or name.upper().startswith(("DATABASE", "PG", "POSTGRES"))]
    assert database == [], facts["env_names"]
    assert facts["secrets"] == ["worker_api_token"]
    assert [mount for mount in facts["mounts"] if "blob" in mount] == [], facts["mounts"]
    assert facts["media_tools"] == []
