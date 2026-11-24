"""Fixtures of the media-pipeline suites (AZ-3661): an account on a fresh install and a helper that commits files and
waits for the real supervisor and decoders to finish with them. The files come from `tests/tools/gen_media.py`
(run.sh makes them in `e2e-results/media`, which the consumer sees as /results/media)."""
import time
from pathlib import Path

import pytest

from stackkit import Browser, Drive, operator_token, restore_and_wait

MEDIA_DIR = Path("/results/media")
ORIGIN = {"Origin": "https://sunnycloud.test"}
JOBS_DONE_SECONDS = 120
POLL_SECONDS = 1.0


class Media:
    """alice, signed in, with her files; every wait reads what the stack shows (jobs, thumbnail states, derived rows)."""

    def __init__(self, ctl):
        self.ctl = ctl
        self.browser = Browser()
        self.browser.sign_in("alice", setup_token=operator_token(ctl, "admin", "setup-token"))
        self.drive = Drive(self.browser)
        self.root = self.drive.root()

    @staticmethod
    def fixture(name):
        return (MEDIA_DIR / name).read_bytes()

    def commit(self, name, data=None):
        """Commits a fixture file (or `data` under `name`); `{"node_id", "content_hash"}`."""
        ref = self.drive.upload(self.root, name, self.fixture(name) if data is None else data)
        status, node = self.browser.api(f"/api/v1/nodes/{ref['node_id']}")
        assert status == 200, node
        return {"node_id": ref["node_id"], "content_hash": node["content_hash"]}

    def state(self, content_hash):
        """The thumbnail state of one file as the listing route reports it; `None` when the account has no job for it."""
        status, body = self.browser.api("/api/v1/thumbnail-states", "POST", {"content_hashes": [content_hash]}, ORIGIN)
        assert status == 200, body
        return body["states"].get(content_hash)

    def jobs(self):
        return self.ctl.call("db-probe", query="media_jobs")

    def derived(self):
        return self.ctl.call("db-probe", query="derived_rows")

    def wait_jobs_closed(self, closed, seconds=JOBS_DONE_SECONDS):
        """Waits until no job is queued or running and `closed` jobs have a terminal state; returns the job counts."""
        deadline = time.monotonic() + seconds
        while True:
            jobs = self.jobs()
            done = sum(jobs[key] for key in ("derived", "skipped_compliant", "obsolete", "dead"))
            if (jobs["queued"] == 0 and jobs["running"] == 0 and done >= closed) or time.monotonic() > deadline:
                return jobs
            time.sleep(POLL_SECONDS)

    def wait_state(self, content_hash, wanted, seconds=JOBS_DONE_SECONDS):
        deadline = time.monotonic() + seconds
        while True:
            seen = self.state(content_hash)
            if seen == wanted or time.monotonic() > deadline:
                return seen
            time.sleep(POLL_SECONDS)

    def derived_file(self, node_id, route):
        """`(status, headers, bytes)` of a derived file route: `thumbnail?size=small`, `thumbnail?size=preview`, `transcode`."""
        return self.browser.fetch(f"/api/v1/nodes/{node_id}/{route}")


@pytest.fixture(scope="module")
def media(stack_ctl):
    """A fresh install with alice signed in; restored again afterwards for the next suite."""
    restore_and_wait(stack_ctl)
    yield Media(stack_ctl)
    restore_and_wait(stack_ctl)
