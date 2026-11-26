"""AZ-3815 AC-2 (AT-12-2, AC-41, AC-65) on the stack: the restore drill an operator runs after restoring a backup.

The backup set is the stack's own: a `pg_dump` of the database (ctl `db-restore` `op: dump`) with the blob volume and
the master key left in place, as a backup of the three restored together. After the dump `admin epoch-bump` runs once,
so the volume's `.epoch` marker is newer than the dump. Then the api is stopped, the dump is loaded and the api started:
`serve` must refuse, naming the bump. The operator commands of the drill run in a one-off api container while `serve`
is down. The device's cursor is its head before the dump; after the drill a poll with it is told to resync.
"""
import json
import time

from stackkit import Browser, operator_token, wait_worker_ok

EXIT_FAILED = 2
STOPPED_SECONDS = 60
READY_SECONDS = 90


def run(ctl, *args, **extra):
    return ctl.call("exec-operator", args=list(args), **extra)


def oneoff(ctl, *args):
    return run(ctl, *args, service="api-oneoff")


def epoch_findings(result):
    envelope = json.loads(result["stdout"])
    return [f for f in envelope["data"]["findings"] if f["check"] == "epoch"]


def wait_stopped(ctl):
    deadline = time.monotonic() + STOPPED_SECONDS
    while time.monotonic() < deadline:
        state = ctl.call("inspect", service="api")
        if state["status"] in ("exited", "dead"):
            return state
        time.sleep(1)
    raise AssertionError(f"serve did not stop within {STOPPED_SECONDS} s: {state}")


def test_at_12_2_serve_refuses_until_the_bump_then_scrub_doctor_and_devices_resync(fresh_install):
    # Arrange
    ctl = fresh_install
    browser = Browser()
    browser.sign_in("alice", setup_token=operator_token(ctl, "admin", "setup-token"))
    status, head = browser.api("/api/v1/changes/head")
    assert status == 200, head
    old_cursor = head["cursor"]
    # The backup of a running install: the worker has reported its decoders healthy (a restored install keeps that
    # row until the worker reports again, which it cannot while `serve` is down).
    wait_worker_ok(ctl)
    ctl.call("db-restore", op="dump", name="ds22")
    first_bump = run(ctl, "admin", "epoch-bump", "--yes", "--json")
    assert first_bump["exit_code"] == 0, first_bump

    # Act
    ctl.call("process-kill", service="api", op="kill")
    ctl.call("db-restore", op="load", name="ds22")
    ctl.call("process-kill", service="api", op="start")
    stopped = wait_stopped(ctl)
    refusal = ctl.call("inspect", logs="api")["log_tail"]
    doctor_before = oneoff(ctl, "doctor", "--json")
    bump = oneoff(ctl, "admin", "epoch-bump", "--yes", "--json")
    scrub = oneoff(ctl, "scrub", "run", "--full")
    doctor_after = oneoff(ctl, "doctor", "--json")
    ctl.call("process-kill", service="api", op="start")
    ready, body = ctl.wait_ready(READY_SECONDS)
    poll_status, problem = browser.api(f"/api/v1/changes?cursor={old_cursor}")

    # Assert
    assert stopped["status"] == "exited", stopped
    assert "admin epoch-bump" in refusal, refusal
    assert doctor_before["exit_code"] == EXIT_FAILED, doctor_before
    assert any(f["level"] == "fail" for f in epoch_findings(doctor_before)), doctor_before
    assert bump["exit_code"] == 0, bump
    new_epoch = json.loads(bump["stdout"])["data"]["epoch"]
    assert new_epoch > json.loads(first_bump["stdout"])["data"]["epoch"], (first_bump, bump)
    assert scrub["exit_code"] == 0, scrub
    assert doctor_after["exit_code"] in (0, 1), doctor_after
    assert all(f["level"] != "fail" for f in epoch_findings(doctor_after)), doctor_after
    assert ready == 200, body
    assert (poll_status, problem["code"], problem["reason"]) == (410, "resync_required", "epoch_changed"), problem
