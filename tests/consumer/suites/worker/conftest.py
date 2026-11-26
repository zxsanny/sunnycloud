"""Fixtures of the worker suite on harness H2 (09 tests.md; AZ-4384, AZ-4385, AZ-3878): the real API, queue and
PostgreSQL, the real supervisor with the H2 timings, and per case either the real decoders (RD) or FD on a lane.

Every case here runs under `tests/run.sh --profile h2` only and is skipped under any other profile."""
import os

import pytest

from fdkit import FakeDecoderLane
from jobkit import Media
from stackkit import restore_and_wait

READY_SECONDS = 90


def pytest_collection_modifyitems(config, items):
    if os.environ.get("E2E_PROFILE") == "h2":
        return
    skip = pytest.mark.skip(reason="the worker suite runs on harness H2: tests/run.sh --profile h2")
    for item in items:
        if "/suites/worker/" in str(item.path):
            item.add_marker(skip)


@pytest.fixture(scope="module")
def media(stack_ctl):
    """A fresh install with alice signed in; restored again afterwards for the next module."""
    restore_and_wait(stack_ctl)
    yield Media(stack_ctl)
    restore_and_wait(stack_ctl)


@pytest.fixture
def lanes(stack_ctl):
    """`take(lane, script, restart_worker=True)` puts FD on a lane; afterwards every lane gets its real decoder back and
    the supervisor restarts once, ready for the next case."""
    taken = []

    def take(lane, script, restart_worker=True):
        fd = FakeDecoderLane(stack_ctl, lane)
        fd.start(script, restart_worker=restart_worker)
        taken.append(fd)
        return fd

    yield take
    for fd in taken:
        fd.stop(restart_worker=False)
    if taken:
        stack_ctl.restart_worker()
        assert stack_ctl.wait_worker_readyz(200, READY_SECONDS) == 200, "the worker is not ready on the real decoders"
