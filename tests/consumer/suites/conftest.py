"""Fixtures of the suites that need an install without accounts (the golden snapshot) and leave one behind."""
import pytest

READY_SECONDS = 90


def restore_and_wait(stack_ctl):
    stack_ctl.call("db-restore", snapshot="golden")
    status, body = stack_ctl.wait_ready(READY_SECONDS)
    assert status == 200, f"the stack is not ready after the restore: {body}"


@pytest.fixture
def fresh_install(stack_ctl):
    """The stack restored to the golden snapshot (no accounts); restored again afterwards for the next suite."""
    restore_and_wait(stack_ctl)
    yield stack_ctl
    restore_and_wait(stack_ctl)
