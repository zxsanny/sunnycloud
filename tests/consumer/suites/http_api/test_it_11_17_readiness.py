"""IT-11-17 (components/11_http_api/tests.md), Mode R, group X: `/readyz` follows the real database and schema (AC-22, AC-65).

These tests change the stack (the database stops, a migration is reverted). Each puts it back in a finalizer, so the
tests after it start on a ready stack; `tests/run.sh reset` is the harness's own restore.
"""
import json
import time

import pytest

RECOVERY_SECONDS = 60
OUTAGE_DETECT_SECONDS = 30
SECRET_MARKERS = ["postgres://", "SELECT ", "/run/secrets", "/var/lib/sunnycloud"]


def failing(body):
    return {check["name"]: check for check in body["checks"] if check["status"] != "ok"}


@pytest.fixture
def restore(stack_ctl):
    yield
    stack_ctl.call("db-outage", op="start")
    stack_ctl.call("interrupted-upgrade", op="apply")
    status, body = stack_ctl.wait_ready(RECOVERY_SECONDS)
    assert status == 200, f"the stack did not come back ready after the test: {body}"


def test_a_ready_stack_answers_200_with_every_check_ok_and_no_secret(stack_ctl):
    # Arrange and Act
    status, body = stack_ctl.wait_ready(RECOVERY_SECONDS)

    # Assert
    assert status == 200 and not failing(body), body
    assert {"database", "migrations", "scheduler_alive"} <= {c["name"] for c in body["checks"]}
    assert not [m for m in SECRET_MARKERS if m in json.dumps(body)]


def test_readiness_is_503_during_a_database_outage_and_200_within_60_s_of_the_database_accepting_connections(stack_ctl, restore):
    # Arrange
    stack_ctl.call("db-outage", op="stop")

    # Act
    deadline = time.monotonic() + OUTAGE_DETECT_SECONDS
    while True:
        status, body = stack_ctl.ready()
        if status == 503 or time.monotonic() > deadline:
            break
        time.sleep(1)
    health = stack_ctl.internal(9090, "/healthz")["status"]
    stack_ctl.call("db-outage", op="start")
    started = time.monotonic()
    recovered_status, recovered = stack_ctl.wait_ready(RECOVERY_SECONDS)
    seconds = time.monotonic() - started

    # Assert
    assert status == 503 and "database" in failing(body), (status, body)
    assert not [m for m in SECRET_MARKERS if m in json.dumps(body)]
    assert health == 200, "/healthz must not depend on the database"
    assert recovered_status == 200, recovered
    assert seconds <= RECOVERY_SECONDS, f"ready again after {seconds:.0f} s"


def test_a_pending_migration_makes_readiness_503_naming_migrations(stack_ctl, restore):
    # Arrange
    assert stack_ctl.ready()[0] == 200

    # Act
    stack_ctl.call("interrupted-upgrade", op="revert", steps=1)
    status, body = stack_ctl.ready()
    health = stack_ctl.internal(9090, "/healthz")["status"]

    # Assert
    assert status == 503 and set(failing(body)) == {"migrations"}, (status, body)
    assert health == 200
    assert not [m for m in SECRET_MARKERS if m in json.dumps(body)]
