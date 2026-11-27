"""IT-09-12 (AC-71, ST-09-3) on H2: decoder output files are hostile input. FD, playing a compromised decoder, puts
`out/result.webp` as a symlink to the worker secret, a FIFO, a sparse file over the cap or a directory: none of them
becomes a derived file, the slot is not blocked, and a regular WebP (the control) is posted.

Leg (b), a hard link to the input, cannot be planted on this host: with `fs.protected_hardlinks = 1` FD (uid 10003) may
not link the supervisor's read-only input; the supervisor's `nlink` check is `src/worker/src/scratch/hostile_tests.rs:98`."""
import time

import pytest

from e2e_report import skip_not_run
from h2cases import PHOTO, WEBP_1X1, chunks, job_rows, photo_probe, requests_of, run_file, wait_for_row

FIFO_SECONDS = 3
POLL_SECONDS = 0.1
SPARSE_OVER_CAP = 64 * 1024 * 1024
HOSTILE = {
    "a-symlink-to-the-secret": {"symlink": {"target": "/run/secrets/worker_api_token"}},
    "c-fifo": "fifo",
    "d-sparse-over-the-cap": {"sparse": {"bytes": SPARSE_OVER_CAP}},
    "f-directory": "directory",
}


def still(output):
    return {"probe": {"default": {"probe": photo_probe()}},
            "render_still": {"default": {"image": {"width": 1, "height": 1, "output": output}}}}


@pytest.mark.parametrize("case", sorted(HOSTILE))
def test_it_09_12_a_hostile_output_file_never_becomes_a_derived_file(stack_ctl, media, lanes, case):
    # Arrange
    derived_before = stack_ctl.call("db-probe", query="derived_rows")["total"]
    rows_before = job_rows(stack_ctl)

    # Act
    fd, _, _, _ = run_file(media, lanes, f"it-09-12-{case}", still(HOSTILE[case]), fixture=PHOTO,
                           wanted=lambda before, after: True)
    deadline = time.monotonic() + 30
    while not requests_of(fd.record(), "render_still") and time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)
    rendered = time.monotonic()
    while media.jobs()["running"] and time.monotonic() - rendered < FIFO_SECONDS:
        time.sleep(POLL_SECONDS)
    left_running = media.jobs()["running"]
    _, ended = wait_for_row(stack_ctl, lambda job, row: job not in rows_before and row["last_error"])
    record = fd.record()

    # Assert
    assert requests_of(record, "render_still"), record
    assert not [line for line in record if line["kind"] == "fd_error"], record
    assert left_running == 0, f"the slot still holds the job {FIFO_SECONDS} s after the hostile output"
    assert (ended or {}).get("last_error") == "output_rejected", ended  # G-09-3 (tests/spec-gaps.md)
    assert stack_ctl.call("db-probe", query="derived_rows")["total"] == derived_before


def test_it_09_12_b_a_hard_link_to_the_input_is_rejected():
    skip_not_run("NO-SERVICE", "fs.protected_hardlinks = 1: FD cannot link the supervisor's read-only input; the nlink "
                               "check is src/worker/src/scratch/hostile_tests.rs:98")


def test_it_09_12_e_a_regular_webp_output_is_posted(stack_ctl, media, lanes):
    # Arrange
    derived_before = stack_ctl.call("db-probe", query="derived_rows")

    # Act
    _, record, before, after = run_file(media, lanes, "it-09-12-e-control", still({"bytes": chunks(WEBP_1X1)}),
                                        fixture=PHOTO, ended=1)

    # Assert
    assert after["derived"] - before["derived"] == 1, (before, after)
    derived = stack_ctl.call("db-probe", query="derived_rows")
    assert derived["total"] > derived_before["total"], (derived_before, derived)
