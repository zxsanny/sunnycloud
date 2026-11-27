"""IT-09-18 (AC-71) on H2, test image: the shared test-clock offset moves exactly one supervisor output,
`image_age_days` (09 description.md, Test clock). FD on both lanes reports a build time 20 days and 1 hour before the
clock.

A clip's transcode is held on the general lane while the clock moves 1 hour. Leases are the API's and follow the offset
(09 description.md:57, contract 08 worker_job_protocol.md:67), so the next heartbeat gets 409 and the supervisor cancels
quietly, as in IT-09-10 A. What the advance must not reach is a supervisor timer: one hour is past every wall limit, so a
watchdog on the offset clock would kill the job; on the monotonic clock there is no kill, the slot stays ready, and the
job is claimed again on the same connection (FD answers that attempt at once). Then the clock moves 3 days with the
supervisor idle: `image_age_days` goes from 20 to 23 within a status interval and one offset poll, and stays there; the
log keeps host time.

Not on the stack: the offset requests themselves (the API logs no 2xx answer; `src/worker/tests/clock_poll.rs`), and the
release image's 0 requests and missing `OffsetClock` (`src/worker/tests/clock_poll_release.rs`, the CI release-symbol
check). The offset only grows, so this is a module of its own; it ends with the golden reset."""
import json
import time
from datetime import datetime

import mediakit
from h2cases import (CLIP, GATE_SECONDS, POLL_SECONDS, heartbeat_gaps, job_rows, lease_samples, new_rows, next_report,
                     photo_probe, postable_image, postable_transcode, requests_of, status_report, video_probe,
                     wait_for_request, wait_for_row)
from stackkit import advance_clock

BUILD_AGE_DAYS = 20
# 20 days and an hour before now: the 1-hour advance leaves the whole-day age at 20, the 3-day one makes it 23.
BUILT = int(time.time()) - BUILD_AGE_DAYS * 86_400 - 3_600
ONE_HOUR, THREE_DAYS = 3_600, 3 * 86_400
HOLD_MS = 30_000
HEARTBEAT_MS, HEARTBEAT_SLACK_MS = 2_000, 500
SAMPLE_SECONDS = 5
EVENT_SECONDS = 20
# One status interval (1 s) and one offset poll (at most 1 s), and the probe's own delay.
AGE_SECONDS = 4
LOG_SLACK_SECONDS = 2
KILLS = "sunnycloud_decoder_kills_total"


def over_time(delay_ms, answer):
    return {"over_time": {"delay_ms": delay_ms, "alive_every_ms": 500, "answer": answer}}


GENERAL = {"hello": {"build_timestamp": BUILT},
           "probe": {"default": {"probe": video_probe(bitrate=12_000_000)}},
           "render_still": {"default": postable_image()},
           "transcode": {"queue": [over_time(HOLD_MS, postable_transcode())], "default": postable_transcode()}}
INTERACTIVE = {"hello": {"build_timestamp": BUILT},
               "probe": {"default": {"probe": photo_probe()}},
               "render_still": {"default": postable_image()}}


def decoder_kills(ctl):
    return sum(value for name, value in ctl.worker_metrics().items() if name.split("{")[0] == KILLS)


def ready(ctl):
    status, _ = ctl.worker_ops("/readyz")
    return status


def age_reaching(ctl, wanted, seconds=AGE_SECONDS):
    """The first status report with `image_age_days == wanted`, and how long it took (or the last one after
    `seconds`)."""
    started = time.monotonic()
    while True:
        report = status_report(ctl)
        if report["image_age_days"] == wanted or time.monotonic() - started > seconds:
            return report, time.monotonic() - started
        time.sleep(POLL_SECONDS)


def newest_log_seconds(ctl):
    """The newest time stamp in the worker's log tail, in seconds since the epoch."""
    tail = ctl.call("inspect", logs="worker")["log_tail"]
    stamps = [datetime.fromisoformat(json.loads(line)["ts"]).timestamp() for line in tail.splitlines()
              if line.startswith("{")]
    return max(stamps)


def test_it_09_18_the_test_clock_moves_only_the_image_age_and_no_supervisor_timer(stack_ctl, media, lanes):
    # Arrange: both slots ready on FD; a clip's transcode held on the general lane.
    interactive = lanes("interactive", INTERACTIVE, restart_worker=False)
    general = lanes("general", GENERAL)
    for fd in (interactive, general):
        fd.wait_for(["hello", "self_check"], GATE_SECONDS)
    rows_before = job_rows(stack_ctl)
    media.commit("it-09-18.mp4", mediakit.with_marker(media.fixture(CLIP), "it-09-18"))
    held = wait_for_request(general, "transcode")
    assert held, general.record()
    before, _ = next_report(stack_ctl, status_report(stack_ctl))
    kills_before = decoder_kills(stack_ctl)
    started = stack_ctl.call("inspect", service="worker")["started_at"]
    gaps = heartbeat_gaps(lease_samples(stack_ctl, SAMPLE_SECONDS))

    # Act 1: one hour, with the job running.
    advance_clock(stack_ctl, ONE_HOUR)
    deadline = time.monotonic() + EVENT_SECONDS
    while len(requests_of(general.record(), "transcode")) < 2 and time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)
    after_hour, _ = next_report(stack_ctl, status_report(stack_ctl))
    ready_after_hour, kills_after_hour = ready(stack_ctl), decoder_kills(stack_ctl)
    record = general.record()
    # The requeued job's second attempt is answered at once: wait for it, so the next advance finds the slot idle.
    wait_for_row(stack_ctl, lambda job, row: job in new_rows(stack_ctl, rows_before) and row["kind"] == "transcode"
                 and row["status"] == "succeeded", seconds=EVENT_SECONDS)

    # Act 2: three days, with the supervisor idle; then the interactive FD restarts, so the supervisor writes a log line
    # (its idle connection ended) after the advance.
    advanced_at = time.time()
    advance_clock(stack_ctl, THREE_DAYS)
    reached, waited = age_reaching(stack_ctl, BUILD_AGE_DAYS + 3)
    stays, _ = next_report(stack_ctl, reached)
    ready_after_days, kills_after_days = ready(stack_ctl), decoder_kills(stack_ctl)
    lanes("interactive", INTERACTIVE, restart_worker=False).wait_for(["hello", "self_check"], GATE_SECONDS)
    newest_log, host_now = newest_log_seconds(stack_ctl), time.time()
    restarts = stack_ctl.call("inspect", service="worker")["started_at"]

    # Assert: before any advance.
    assert before["image_age_days"] == BUILD_AGE_DAYS, before
    assert gaps and all(abs(gap - HEARTBEAT_MS) <= HEARTBEAT_SLACK_MS for gap in gaps), gaps
    # The hour: the API's lease is gone (cancel, the job claimed again on the same connection), no supervisor timer fired.
    transcodes, cancels = requests_of(record, "transcode"), requests_of(record, "cancel")
    assert cancels and cancels[0]["at_ms"] - transcodes[0]["at_ms"] < HOLD_MS, record
    assert len(transcodes) == 2 and transcodes[1]["job"] == transcodes[0]["job"], record
    assert transcodes[1]["conn"] == transcodes[0]["conn"], record
    assert (ready_after_hour, kills_after_hour) == (200, kills_before), (ready_after_hour, kills_before, kills_after_hour)
    assert after_hour["image_age_days"] == BUILD_AGE_DAYS, after_hour
    # The three days: the age moves by exactly 3 within one interval and one poll, and stays.
    assert (reached["image_age_days"], stays["image_age_days"]) == (BUILD_AGE_DAYS + 3,) * 2, (reached, stays)
    assert stays["seen_ms"] > reached["seen_ms"], (reached, stays)
    assert waited <= AGE_SECONDS, waited
    assert (ready_after_days, kills_after_days) == (200, kills_before), (ready_after_days, kills_after_days)
    assert advanced_at - LOG_SLACK_SECONDS <= newest_log <= host_now + LOG_SLACK_SECONDS, (advanced_at, newest_log, host_now)
    assert restarts == started, "the supervisor restarted: its kill counter would read 0 again"
