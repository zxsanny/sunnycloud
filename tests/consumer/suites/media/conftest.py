"""Fixtures of the media-pipeline suites (AZ-3661): an account on a fresh install and a helper that commits files and
waits for the real supervisor and decoders to finish with them (`jobkit.Media`)."""
import pytest

from jobkit import Media
from stackkit import restore_and_wait


@pytest.fixture(scope="module")
def media(stack_ctl):
    """A fresh install with alice signed in; restored again afterwards for the next suite."""
    restore_and_wait(stack_ctl)
    yield Media(stack_ctl)
    restore_and_wait(stack_ctl)
