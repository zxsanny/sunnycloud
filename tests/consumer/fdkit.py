"""FD, the scripted fake decoder of harness H2 (`tests/run.sh --profile h2`), from a suite's side.

A lane's FD reads `/results/fd/<lane>/script.json` when it starts and appends every message it receives to
`record.jsonl` there (formats: the module doc of `src/worker/fake-decoder`). `start` writes the script, removes the
last record and has e2e-ctl put FD in place of that lane's real decoder; `stop` puts the real decoder back. FD is for
fault and protocol cases only, never for a case that claims a media outcome.

Both restart the supervisor by default (SIGKILL, then start), so it starts on the swapped socket at once and the case
does not wait out its reconnect backoff (1 s doubling to 30 s). A case about the reconnect itself (IT-09-15) passes
`restart_worker=False`.
"""
import json
import os
import time
from pathlib import Path

LANES = {"interactive": "decoder-interactive", "general": "decoder-general"}
POLL_SECONDS = 0.2
# A `self_check` with one failed item: the supervisor's gate refuses the slot, so it claims nothing on that lane.
FAILED_SELF_CHECK = [{"name": "no_network", "ok": False}]
# The record kinds of job requests (the others are `hello`, `self_check`, `cancel` and FD's own events).
JOB_REQUESTS = frozenset({"probe", "render_still", "transcode"})


class FakeDecoderLane:
    def __init__(self, ctl, lane):
        if lane not in LANES:
            raise ValueError(f"lane is one of {sorted(LANES)}, not {lane!r}")
        self.ctl = ctl
        self.decoder = LANES[lane]
        self.service = f"fd-{lane}"
        self.dir = Path(os.environ.get("E2E_RESULTS_DIR", "/results")) / "fd" / lane

    @property
    def record_path(self):
        return self.dir / "record.jsonl"

    def start(self, script, restart_worker=True):
        if not isinstance(script, dict):
            raise ValueError("the script is a JSON object")
        (self.dir / "script.json").write_text(json.dumps(script), encoding="utf-8")
        self.record_path.unlink(missing_ok=True)
        self.ctl.call("process-kill", service=self.decoder, op="fake")
        if restart_worker:
            self._restart_worker()

    def stop(self, restart_worker=True):
        self.ctl.call("process-kill", service=self.decoder, op="real")
        if restart_worker:
            self._restart_worker()

    def _restart_worker(self):
        self.ctl.restart_worker()

    def record(self):
        """The record lines so far; a last line FD is still writing is left for the next read."""
        if not self.record_path.exists():
            return []
        lines = self.record_path.read_text(encoding="utf-8").split("\n")
        return [json.loads(line) for line in lines[:-1] if line]

    def wait_for(self, kinds, seconds):
        """Waits until the record holds `kinds` in this order (other lines between them allowed); returns the record.
        Raises when they do not show within `seconds`."""
        deadline = time.monotonic() + seconds
        while True:
            record = self.record()
            if contains_in_order([line["kind"] for line in record], kinds):
                return record
            if time.monotonic() > deadline:
                raise AssertionError(f"FD on {self.decoder} did not see {kinds} within {seconds} s: {record}; "
                                     f"FD log: {self.log_tail()}")
            time.sleep(POLL_SECONDS)

    def log_tail(self):
        """FD's stderr (a script it refused, a socket it could not bind), for a failure message."""
        status, body = self.ctl.try_call("inspect", logs=self.service)
        return body.get("result", {}).get("log_tail", "") if status == 200 else f"(no log: {status} {body})"


def job_requests(fd):
    """The job requests in a lane's record."""
    return [line for line in fd.record() if line["kind"] in JOB_REQUESTS]


def contains_in_order(seen, wanted):
    position = 0
    for kind in seen:
        if position < len(wanted) and kind == wanted[position]:
            position += 1
    return position == len(wanted)
