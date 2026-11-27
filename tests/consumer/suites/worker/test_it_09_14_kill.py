"""IT-09-14 (AC-71, R4) on H2 with the real decoders: a supervisor killed (SIGKILL) during the input fetch, during the
transcode and during the output post leaves a clean end state after its restart.

Each kill lands inside its step by construction. The fetch and the post are parked by the API test image's one-shot
transfer hold (AZ-4381, decision D1) until the kill; the transcode runs on a decoder held to a fifth of a CPU (e2e-ctl
`cpu-limit`, decision D4), and the kill comes while its ffmpeg is in the decoder's process list. After the kill the
killed run's ffmpeg is gone within 2 s (the decoder cancels on the lost socket); the supervisor starts again with both
decoders frozen, so no slot can pass its gate and claim, and both scratch roots must be empty then (swept at start);
the decoders thaw, the test clock moves past the 6 s lease, only after the kill (a lease lost first would prove
IT-09-10 instead), and the job is claimed again (attempt 2) and succeeds. The post leg holds the photo's thumbnail, so its preview is already stored and attempt 2's
preview is answered "already present", which the supervisor takes as success. End state: one derived row per kind, no
derived blob nothing refers to, empty scratch, no ffmpeg."""
import time

import pytest

import mediakit
from h2cases import JOBS_SECONDS, PHOTO, POLL_SECONDS, arm_hold, job_rows, new_rows, release_hold, wait_for_row, wait_held
from stackkit import advance_clock

READY_SECONDS = 90
PAST_THE_LEASE_SECONDS = 10
CANCEL_SECONDS = 2
JANITOR_SECONDS = 30
# Half a CPU: the transcode runs for seconds, and the runner still answers inside H2's 1 s silence timeout (at 0.2
# every attempt ended `DecoderSilent`). Only the killed attempt runs this slow: the quota is set back right after the kill.
SLOW_CPUS = 0.5
HOLD_AFTER_BYTES = 16 * 1024
LANES = ("interactive", "general")
DECODERS = ("decoder-interactive", "decoder-general")
NONCOMPLIANT = "clip-noncompliant.mp4"


class SlowDecoder:
    """decoder-general's CPU quota lowered by `limit` and set back by `restore` (also at the end of the case)."""

    def __init__(self, ctl):
        self.ctl = ctl
        self.previous = None

    def limit(self, cpus):
        answer = self.ctl.call("cpu-limit", service="decoder-general", cpus=cpus)
        if self.previous is None:
            self.previous = answer["previous_cpus"]

    def restore(self):
        if self.previous is not None:
            self.ctl.call("cpu-limit", service="decoder-general", cpus=self.previous)
            self.previous = None


@pytest.fixture
def slow_decoder(stack_ctl):
    decoder = SlowDecoder(stack_ctl)
    yield decoder
    decoder.restore()


@pytest.fixture(autouse=True)
def stack_whole_again(stack_ctl):
    """Whatever a case leaves (a parked stream, a dead supervisor), the next one starts with both running."""
    yield
    release_hold(stack_ctl)
    if stack_ctl.call("inspect", service="worker")["status"] != "running":
        stack_ctl.call("process-kill", service="worker", op="start")
    assert stack_ctl.wait_worker_readyz(200, READY_SECONDS) == 200


def ffmpegs(ctl):
    """The ffmpeg processes of both decoders: `(decoder, args)`."""
    return [(decoder, p["args"]) for decoder in DECODERS for p in ctl.call("inspect", processes=decoder)["processes"]
            if p["comm"] == "ffmpeg"]


def scratch(ctl):
    return {lane: ctl.call("inspect", scratch=lane)["entries"] for lane in LANES}


def wait_until(read, done, seconds):
    deadline = time.monotonic() + seconds
    while True:
        seen = read()
        if done(seen) or time.monotonic() > deadline:
            return seen
        time.sleep(POLL_SECONDS)


def restart_and_retry(ctl, job, rows_before):
    """Starts the supervisor again with both decoders frozen, so no slot can pass its gate and claim: the scratch it
    swept at start is read then, with the killed job's row. Then the decoders thaw, the clock moves past the lease and
    every job of the file must succeed. Returns that scratch read, that row and the file's rows at the end."""
    for decoder in DECODERS:
        ctl.call("process-kill", service=decoder, op="pause")
    try:
        ctl.call("process-kill", service="worker", op="start")
        swept = wait_until(lambda: scratch(ctl), lambda seen: seen == {lane: [] for lane in LANES}, 15)
        leased = job_rows(ctl)[job]
    finally:
        for decoder in DECODERS:
            ctl.call("process-kill", service=decoder, op="unpause")
    assert ctl.wait_worker_readyz(200, READY_SECONDS) == 200
    advance_clock(ctl, PAST_THE_LEASE_SECONDS)
    wait_for_row(ctl, lambda seen, row: seen == job and row["status"] == "succeeded", seconds=JOBS_SECONDS * 2)
    rows = wait_until(lambda: new_rows(ctl, rows_before), lambda seen: all(r["status"] == "succeeded" for r in seen.values()),
                      JOBS_SECONDS)
    return swept, leased, rows


def assert_clean_end(ctl, media, job, swept, leased, rows, derived_before, expected):
    assert swept == {lane: [] for lane in LANES}, swept
    assert leased["attempts"] == 1, f"the killed job was claimed again before the sweep: {leased}"
    assert all(row["status"] == "succeeded" for row in rows.values()), rows
    assert rows[job]["attempts"] == 2, rows
    derived = media.derived()
    assert {kind: derived[kind] - derived_before[kind] for kind in expected} == expected, (derived_before, derived)
    refs = wait_until(lambda: ctl.call("db-probe", query="blob_refs"), lambda r: r["unreferenced"] == 0, JANITOR_SECONDS)
    assert refs["unreferenced"] == 0, refs
    assert scratch(ctl) == {lane: [] for lane in LANES}
    assert ffmpegs(ctl) == []


def test_it_09_14_a_kill_during_the_input_fetch(stack_ctl, media):
    # Arrange
    derived_before, rows_before = media.derived(), job_rows(stack_ctl)
    arm_hold(stack_ctl, "input", HOLD_AFTER_BYTES)

    # Act
    media.commit("it-09-14-fetch.jpg", mediakit.with_marker(media.fixture(PHOTO), "it-09-14-fetch"))
    held = wait_held(stack_ctl)
    stack_ctl.call("process-kill", service="worker", op="kill")
    release_hold(stack_ctl)
    swept, leased, rows = restart_and_retry(stack_ctl, held["job_id"], rows_before)

    # Assert
    assert held["held"] and held["bytes"] >= HOLD_AFTER_BYTES, held
    assert_clean_end(stack_ctl, media, held["job_id"], swept, leased, rows, derived_before,
                     {"thumb": 1, "preview": 1, "transcode": 0})


def test_it_09_14_b_kill_during_the_transcode(stack_ctl, media, slow_decoder):
    # Arrange
    derived_before, rows_before = media.derived(), job_rows(stack_ctl)
    slow_decoder.limit(SLOW_CPUS)

    # Act
    media.commit("it-09-14-decode.mp4", mediakit.with_marker(media.fixture(NONCOMPLIANT), "it-09-14-decode"))
    running = wait_until(lambda: [args for _, args in ffmpegs(stack_ctl) if "result.mp4" in args], bool, JOBS_SECONDS)
    stack_ctl.call("process-kill", service="worker", op="kill")
    killed = time.monotonic()
    left = wait_until(lambda: ffmpegs(stack_ctl), lambda seen: seen == [], CANCEL_SECONDS * 3)
    cancelled_after = time.monotonic() - killed
    slow_decoder.restore()
    job = next((job for job, row in new_rows(stack_ctl, rows_before).items() if row["kind"] == "transcode"), None)
    swept, leased, rows = restart_and_retry(stack_ctl, job, rows_before)

    # Assert
    assert running, "no transcode ffmpeg was seen in the decoder"
    assert left == [] and cancelled_after <= CANCEL_SECONDS, (left, cancelled_after)
    assert_clean_end(stack_ctl, media, job, swept, leased, rows, derived_before,
                     {"thumb": 1, "preview": 0, "transcode": 1})


def test_it_09_14_c_kill_during_the_output_post(stack_ctl, media):
    # Arrange: the thumbnail is posted after the preview, so attempt 2 finds the preview already stored.
    derived_before, rows_before = media.derived(), job_rows(stack_ctl)
    arm_hold(stack_ctl, "output", HOLD_AFTER_BYTES // 16, kind="thumb")

    # Act
    media.commit("it-09-14-post.jpg", mediakit.with_marker(media.fixture(PHOTO), "it-09-14-post"))
    held = wait_held(stack_ctl)
    stack_ctl.call("process-kill", service="worker", op="kill")
    release_hold(stack_ctl)
    swept, leased, rows = restart_and_retry(stack_ctl, held["job_id"], rows_before)

    # Assert
    assert held["held"], held
    assert_clean_end(stack_ctl, media, held["job_id"], swept, leased, rows, derived_before,
                     {"thumb": 1, "preview": 1, "transcode": 0})
