"""ST-09-1 (AC-79, AC-25) on H2: the worker secret appears nowhere but the Authorization header. The case runs the flows
of IT-09-2 (a photo job on the real decoders), IT-09-13 (a hostile frame: a protocol violation, the job failed and the
slot gated again) and IT-09-10 case B (the API refuses the secret: 403, the worker not ready), then has e2e-ctl count
the worker tokens in the logs of the stack's services (api, worker, decoders, caddy, db and the rest of its log list),
in the audit rows, in the host's process list, and in what the consumer read: the worker's `/metrics` and `/readyz`
bodies, every status report row (its free-text worker id included), every job's failure text, and FD's record of
every message the supervisor sent its decoder.

The marker is the stack's own worker token, 48 characters (`tests/run.sh`: `openssl rand -hex 24`), and the tokens
rotations retired, which a refused worker still sends. ctl reads them from the run's secrets and returns counts only:
no token reaches the consumer. One hostile frame is enough: every frame kind ends in the same protocol-violation path
of the decoder client, which never holds the token (the API client does). Its own module, so it starts from a golden
reset; the `lanes` fixture restarts the worker on the rotated token for the next module."""
import json
import time

import mediakit
from h2cases import (PHOTO, POLL_SECONDS, job_rows, readiness_item, run_file, terminal, wait_for_row)
from jobkit import JOBS_DONE_SECONDS

EVENT_SECONDS = 20
OVERSIZE_FRAME = (65_537).to_bytes(4, "big") + b" " * 65_537


def test_st_09_1_the_worker_secret_is_in_no_log_metric_readiness_body_or_status_report(stack_ctl, media, lanes):
    # Arrange: IT-09-2, a photo job on the real decoders.
    before = media.jobs()
    media.commit("st-09-1.jpg", mediakit.with_marker(media.fixture(PHOTO), "st-09-1"))
    photo_jobs = media.wait_jobs_closed(terminal(before) + 1, JOBS_DONE_SECONDS)
    # IT-09-13: a frame over the size limit from FD on the general lane.
    rows_before = job_rows(stack_ctl)
    fd, _, _, _ = run_file(media, lanes, "st-09-1-frame", {"probe": {"default": {"garbage": {
        "framed": False, "chunks": [{"hex": OVERSIZE_FRAME.hex()}]}}}}, fixture=PHOTO, wanted=lambda before, after: True)
    _, violated = wait_for_row(stack_ctl, lambda job, row: job not in rows_before and row["last_error"])
    # IT-09-10 case B: the API drops the token the worker holds.
    stack_ctl.rotate_worker_token("drop-old")
    deadline = time.monotonic() + EVENT_SECONDS
    while readiness_item(stack_ctl, "worker_api") != "failing" and time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)
    refused = readiness_item(stack_ctl, "worker_api")

    # Act
    status_rows = stack_ctl.call("db-probe", query="worker_status_rows")["values"]
    shown = [stack_ctl.worker_ops("/metrics")[1], stack_ctl.worker_ops("/readyz")[1], json.dumps(status_rows),
             json.dumps(job_rows(stack_ctl)), json.dumps(fd.record())]
    counts = stack_ctl.call("inspect", secret_scan={"tokens": [], "outputs": "\n".join(shown)})["counts"]

    # Assert
    assert photo_jobs["derived"] - before["derived"] == 1, (before, photo_jobs)
    assert violated, "the hostile frame did not fail a job"
    assert refused == "failing", "the API did not refuse the worker's secret"
    assert status_rows and all(shown[:2]), "nothing to scan: no status report, or the worker's ops port did not answer"
    assert {"worker_token", "worker_token_retired_0"} <= set(counts["logs"]), sorted(counts["logs"])
    leaks = {place: {label: n for label, n in found.items() if label.startswith("worker_token") and n}
             for place, found in counts.items()}
    assert {place: found for place, found in leaks.items() if found} == {}
