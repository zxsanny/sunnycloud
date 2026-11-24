"""AZ-3665 (flow F13) on the stack: an account is deleted through the real routes, the real scheduler and the real
purger, and nothing of it is left. Two accounts: `alice`, the admin, who owns no data, and `bob`, a user with a file
and its thumbnail job, a share link, a staged upload, an open upload session, a device token and a held long-poll.

The stack sets `account_delete_interval` to 8 s and `gc_interval` to 5 s (`tests/compose.test.yml`), so a purge is
done in about a quarter of a minute. "No row of the account remains" is read as: every table the data lives in is back
to what it held before bob existed (`table_rows`), with the audit log keeping one id-only entry.
"""
import base64
import concurrent.futures
import time
import uuid

import pytest

from stackkit import Browser, Drive, device_token, operator_token, restore_and_wait

PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
)
ACCESS_ENDS_SECONDS = 5
PURGE_SECONDS = 90
SECRET_KEYS = {"token", "code", "secret", "password", "email", "name", "refresh", "access", "invite", "setup_token"}


def rows(ctl):
    return ctl.call("db-probe", query="table_rows")


def wait_for(ctl, expected, seconds=PURGE_SECONDS):
    """Polls the table counts until they equal `expected` (the purger and the sweeper run on their intervals)."""
    deadline = time.monotonic() + seconds
    while True:
        seen = rows(ctl)
        if seen == expected or time.monotonic() > deadline:
            return seen
        time.sleep(2)


class World:
    """alice (admin, no data) and bob (user, with data), on a fresh install."""

    def __init__(self, ctl):
        self.ctl = ctl
        self.alice = Browser()
        self.alice.sign_in("alice", setup_token=operator_token(ctl, "admin", "setup-token"))
        self.alice_id = self.alice.api("/api/v1/accounts/me")[1]["id"]
        self.baseline = rows(ctl)
        invite = operator_token(ctl, "admin", "invite", "--admin", self.alice_id, "create")
        self.bob = Browser()
        self.bob.sign_in("bob", invite_token=invite)
        self.bob_id = self.bob.api("/api/v1/accounts/me")[1]["id"]
        self.drive = Drive(self.bob)

    def give_bob_data(self):
        root = self.drive.root()
        picture = self.drive.upload(root, "pic.png", PNG)
        self.drive.ok(201, "/api/v1/shares", "POST", {"node_id": picture["node_id"], "expires_in": 60})
        self.drive.ok(200, "/api/v1/uploads", "POST", {"parent_id": root, "name": "big.bin", "declared_size": 3 * 1024 * 1024,
                                                      "client_key": str(uuid.uuid4())})
        staged = str(uuid.uuid4())
        self.drive.ok(200, f"/api/v1/staged-uploads/{staged}", "PUT", b"staged and never committed", self._hash(b"staged and never committed"))
        return root, staged

    @staticmethod
    def _hash(data):
        from blake3 import blake3

        return {"X-Blake3": blake3(data).hexdigest()}

    def delete_bob(self, browser=None):
        return Drive(browser or self.bob).call("/api/v1/accounts/me", "DELETE", {"confirmation": self.bob_id})


@pytest.fixture
def world(stack_ctl):
    restore_and_wait(stack_ctl)
    yield World(stack_ctl)
    restore_and_wait(stack_ctl)


def test_access_ends_at_once_when_the_deletion_is_confirmed(world):
    # Arrange
    root, staged = world.give_bob_data()
    desktop = device_token(world.bob)
    cursor = world.bob.api("/api/v1/changes/head")[1]["cursor"]
    nodes_before = rows(world.ctl)["nodes"]
    pool = concurrent.futures.ThreadPoolExecutor(max_workers=1)
    held = pool.submit(lambda: (Browser.api(world.bob, f"/api/v1/changes?cursor={cursor}&wait=30"), time.monotonic()))
    time.sleep(1)  # the poll is held on the server before the deletion starts

    # Act
    status, accepted = world.delete_bob()
    asked = time.monotonic()
    (poll_status, _), ended = held.result(timeout=ACCESS_ENDS_SECONDS + 5)
    device = Browser().api("/api/v1/accounts/me", headers={"Authorization": f"Bearer {desktop}"})[0]
    commit = world.drive.call("/api/v1/commits:batch", "POST", {
        "commit_id": str(uuid.uuid4()), "entries": [{"staged_key": staged, "parent_id": root, "name": "late.txt"}]})
    pool.shutdown()

    # Assert
    assert status == 202 and "deadline" in accepted, (status, accepted)
    assert poll_status == 401 and ended - asked <= ACCESS_ENDS_SECONDS, (poll_status, ended - asked)
    assert device == 401, "the device token still works"
    assert commit[0] in (401, 403), f"a pending commit was not refused: {commit}"
    assert rows(world.ctl)["nodes"] <= nodes_before, "a commit landed after the deletion was requested"


def test_everything_of_the_account_is_purged_and_one_id_only_audit_entry_remains(world):
    # Arrange
    world.give_bob_data()
    device_token(world.bob)
    assert rows(world.ctl) != world.baseline, "the data was not created"

    # Act
    status, _ = world.delete_bob()
    seen = wait_for(world.ctl, world.baseline)
    events = world.ctl.call("db-probe", query="audit_events")
    keys = world.ctl.call("db-probe", query="audit_detail_keys")["values"]
    later = Browser()
    location = later.sign_in("bob")

    # Assert
    assert status == 202
    assert seen == world.baseline, {k: (seen[k], world.baseline[k]) for k in seen if seen[k] != world.baseline[k]}
    assert events["account_deleted"] == 1, events
    assert not SECRET_KEYS & set(keys), f"the audit detail names a secret or a person: {keys}"
    assert "invitation_required" in location, "a deleted identity signed in without an invite"


def test_a_purge_that_is_cut_by_a_crash_resumes_from_the_deleting_state(world):
    # Arrange
    world.give_bob_data()
    status, _ = world.delete_bob()

    # Act
    world.ctl.call("process-kill", service="api", op="kill")
    states = world.ctl.call("db-probe", query="account_states")
    world.ctl.call("process-kill", service="api", op="start")
    ready, _ = world.ctl.wait_ready(60)
    seen = wait_for(world.ctl, world.baseline)
    events = world.ctl.call("db-probe", query="audit_events")

    # Assert
    assert status == 202
    if states["deleting"] == 0:
        pytest.skip("NOT-RUN(NO-CRASH-WINDOW): the purge finished before the kill; run again")
    assert ready == 200
    assert seen == world.baseline
    assert events["account_deleted"] == 1, f"the deletion was recorded {events['account_deleted']} times"


def test_the_last_admin_cannot_delete_itself_and_a_device_token_cannot_delete_an_account(world):
    # Arrange
    desktop = device_token(world.bob)
    admin_drive = Drive(world.alice)

    # Act
    last_admin = admin_drive.call("/api/v1/accounts/me", "DELETE", {"confirmation": world.alice_id})
    by_device = Browser().api("/api/v1/accounts/me", "DELETE", {"confirmation": world.bob_id},
                              {"Authorization": f"Bearer {desktop}"})
    wrong_confirmation = world.drive.call("/api/v1/accounts/me", "DELETE", {"confirmation": "yes please"})
    states = world.ctl.call("db-probe", query="account_states")

    # Assert
    assert last_admin[0] == 409 and last_admin[1]["code"] == "last_admin", last_admin
    assert by_device[0] == 403, by_device
    assert 400 <= wrong_confirmation[0] < 500, wrong_confirmation
    assert states == {"active": 2, "deleting": 0}, states
