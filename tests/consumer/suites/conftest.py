"""Fixtures of the suites that need an install without accounts (the golden snapshot) and leave one behind."""
import pytest

from stackkit import restore_and_wait


@pytest.fixture
def fresh_install(stack_ctl):
    """The stack restored to the golden snapshot (no accounts); restored again afterwards for the next suite."""
    restore_and_wait(stack_ctl)
    yield stack_ctl
    restore_and_wait(stack_ctl)
