"""IT-09-7 (AC-72, AC-25) on H2: the supervisor's input fetch on the real API. It hashes while it streams and refuses a
hash mismatch, an input over its limit and a job the scratch mount cannot hold before the decoder sees anything; a clean
input reaches the decoder as an exact, read-only file whose hash is the job's.

FD answers on the general lane with the interactive slot gated out. Legs (b) and (e) run the worker as a variant
(e2e-ctl `worker-variant`) and recreate it as the stack defines it afterwards. Leg (c) is waived on the stack
(decision D2b, 2026-11-26)."""
import pytest

import mediakit
from e2e_report import skip_not_run
from fdkit import job_requests
from h2cases import (CLEAN_PHOTO, PHOTO, fd_on_general, gate_out_both_lanes, job_rows, settled_scratch,
                     wait_for_request, wait_for_row)

READY_SECONDS = 90
DECODER_GID = 10003
SOURCE_MODE = 0o440
# The most one small upload holds, and over the 1 MiB photo limit of the variant `photo-limit-1mib`.
TWO_MIB = 2 * 1024 * 1024


@pytest.fixture
def worker_variant(stack_ctl):
    """`use(variant)` recreates the worker with that variant; afterwards it is recreated as the stack defines it. A case
    asks for it before `lanes`, so the real decoders are back first."""
    used = []

    def use(variant):
        used.append(variant)  # before the call: a recreate whose health wait fails is still undone
        stack_ctl.call("worker-variant", variant=variant)

    yield use
    if used:
        stack_ctl.call("worker-variant", variant="default")
        assert stack_ctl.wait_worker_readyz(200, READY_SECONDS) == 200, "the default worker is not ready"


def photo(media, name, size=None):
    """The fixture photo with a unique tail (its own content hash), padded to `size` bytes when given."""
    data = media.fixture(PHOTO)
    return mediakit.with_marker(data, name if size is None else name.ljust(size - len(data), "x"))


def new_job(ctl, before):
    """The id of the first job that is not in `before`."""
    job, _ = wait_for_row(ctl, lambda job, row: job not in before)
    assert job, "the committed photo got no job"
    return job


def ended(ctl, job):
    """The job's row once an attempt of it ended (a `last_error`, or it succeeded)."""
    _, row = wait_for_row(ctl, lambda seen, row: seen == job and (row["last_error"] or row["status"] == "succeeded"))
    return row


def test_it_09_7_a_a_job_whose_hash_differs_from_its_input_fails_retry_before_the_decoder_sees_it(stack_ctl, media,
                                                                                                  lanes):
    # Arrange: the job waits queued (both lanes gated out) while the harness edits its hash.
    gate_out_both_lanes(stack_ctl, lanes)
    before = job_rows(stack_ctl)
    media.commit("it-09-7-a.jpg", photo(media, "it-09-7-a"))
    job = new_job(stack_ctl, before)
    edited = stack_ctl.call("job-row-edit", job=job)["edited"]

    # Act: FD passes the general gate now, and the supervisor claims the job.
    fd = lanes("general", CLEAN_PHOTO)
    failed = ended(stack_ctl, job)
    scratch = settled_scratch(stack_ctl, job)

    # Assert
    assert edited == 1
    assert (failed["last_error"], failed["status"] != "dead") == ("HashMismatch", True), failed
    assert job_requests(fd) == []
    assert scratch == []


def test_it_09_7_b_a_photo_over_the_input_limit_is_refused_from_its_declared_length_and_dies(stack_ctl, media,
                                                                                             worker_variant, lanes):
    # Arrange
    worker_variant("photo-limit-1mib")
    fd = fd_on_general(lanes, CLEAN_PHOTO)
    before = job_rows(stack_ctl)

    # Act
    media.commit("it-09-7-b.jpg", photo(media, "it-09-7-b", TWO_MIB))
    job = new_job(stack_ctl, before)
    failed = ended(stack_ctl, job)
    scratch = settled_scratch(stack_ctl, job)

    # Assert
    assert (failed["last_error"], failed["status"], failed["attempts"]) == ("TooLarge", "dead", 1), failed
    assert job_requests(fd) == []
    assert scratch == []


def test_it_09_7_c_a_body_past_its_declared_length_is_aborted_at_the_limit():
    skip_not_run("NO-SERVICE", "the real API always declares an exact Content-Length (contract 08 worker_job_protocol.md); "
                               "waived on the stack by decision D2b (2026-11-26); the client-side abort of a body with no "
                               "declared length is src/worker/tests/api_input.rs:175")


def test_it_09_7_d_a_clean_photo_reaches_the_decoder_as_an_exact_read_only_file_whose_hash_is_the_jobs(stack_ctl, media,
                                                                                                       lanes):
    # Arrange
    fd = fd_on_general(lanes, CLEAN_PHOTO)
    before = job_rows(stack_ctl)
    data = photo(media, "it-09-7-d")

    # Act
    committed = media.commit("it-09-7-d.jpg", data)
    probes = wait_for_request(fd, "probe")
    closed = ended(stack_ctl, new_job(stack_ctl, before))

    # Assert
    assert len(probes) == 1, fd.record()
    assert probes[0]["input"] == {"bytes": len(data), "mode": SOURCE_MODE, "gid": DECODER_GID,
                                  "hash": committed["content_hash"]}
    assert closed["status"] == "succeeded", closed


def test_it_09_7_e_a_job_the_scratch_mount_cannot_hold_is_refused_before_download(stack_ctl, media, worker_variant,
                                                                                  lanes):
    # Arrange: the general slot's scratch is an 8 MiB tmpfs, less than the 256 MiB photo input limit.
    worker_variant("small-scratch-general")
    fd = fd_on_general(lanes, CLEAN_PHOTO)
    before, derived = job_rows(stack_ctl), media.derived()["total"]

    # Act
    media.commit("it-09-7-e.jpg", photo(media, "it-09-7-e"))
    failed = ended(stack_ctl, new_job(stack_ctl, before))

    # Assert: `input_too_large`, permanent (gap G-09-2, resolved in tests/spec-gaps.md).
    assert (failed["last_error"], failed["status"]) == ("input_too_large", "dead"), failed
    assert job_requests(fd) == []
    assert media.derived()["total"] == derived
