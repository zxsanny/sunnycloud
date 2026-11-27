"""IT-09-17 (AC-45, AC-71) on H2: SIGTERM drains within `worker_shutdown_grace` (3 s on H2), and the status report
carries decoder health and the image age.

FD on both lanes reports a build time 20 days before the clock. The general lane holds a clip's transcode for 8 s; the
interactive lane renders a photo in about 2 s; a second photo, committed just before the stop, waits queued while
both slots are busy. Then the
supervisor gets SIGTERM (e2e-ctl `process-kill` `stop`, SIGKILL only after 10 s): the 1 s job finishes and posts, the
8 s job gets `cancel` at the grace and its lease is released, the queued photo is never claimed, and the process exits
0 before the stop's SIGKILL could land."""
import time

import mediakit
from fdkit import job_requests
from h2cases import (CLIP, GATE_SECONDS, PHOTO, job_rows, new_rows, next_report, photo_probe, postable_image,
                     postable_transcode, requests_of, status_report, video_probe, wait_for_request, wait_for_row)

BUILD_AGE_DAYS = 20
# 20 days and an hour before now: the whole-day age is 20 for the rest of the run.
BUILT = int(time.time()) - BUILD_AGE_DAYS * 86_400 - 3_600
SHUTDOWN_GRACE_SECONDS = 3
EXIT_WITHIN_SECONDS = 5
HOLD_MS = 8_000
# Each of the photo's two renders: the job (about 2 s) outlasts the commit and the stop call that follow its first
# render, so the stop finds it running, and it still ends well inside the 3 s grace.
RENDER_MS = 1_000
STATUS_INTERVAL_SECONDS = 1
CLOCK_SLACK_MS = 2_000


def over_time(delay_ms, answer):
    return {"over_time": {"delay_ms": delay_ms, "alive_every_ms": 250, "answer": answer}}


GENERAL = {"hello": {"build_timestamp": BUILT},
           "probe": {"default": {"probe": video_probe(bitrate=12_000_000)}},
           "render_still": {"default": postable_image()},
           "transcode": {"default": over_time(HOLD_MS, postable_transcode())}}
INTERACTIVE = {"hello": {"build_timestamp": BUILT},
               "probe": {"default": {"probe": photo_probe()}},
               "render_still": {"default": over_time(RENDER_MS, postable_image())}}


def claimed(fds):
    """The jobs FD received requests for, per lane."""
    return [sorted({line["job"] for line in job_requests(fd)}) for fd in fds]


def test_it_09_17_sigterm_drains_within_the_grace_and_the_status_carries_health_and_image_age(stack_ctl, media, lanes):
    # Arrange: the clip's transcode is held on the general lane, the first photo renders on the interactive lane.
    interactive = lanes("interactive", INTERACTIVE, restart_worker=False)
    general = lanes("general", GENERAL)
    for fd in (interactive, general):
        fd.wait_for(["hello", "self_check"], GATE_SECONDS)
    rows_before = job_rows(stack_ctl)
    media.commit("it-09-17.mp4", mediakit.with_marker(media.fixture(CLIP), "it-09-17"))
    assert wait_for_request(general, "transcode"), general.record()
    first = status_report(stack_ctl)
    second, waited = next_report(stack_ctl, first)
    host_ms = int(time.time() * 1000)
    media.commit("it-09-17-a.jpg", mediakit.with_marker(media.fixture(PHOTO), "it-09-17-a"))
    assert wait_for_request(interactive, "render_still"), interactive.record()
    media.commit("it-09-17-b.jpg", mediakit.with_marker(media.fixture(PHOTO), "it-09-17-b"))
    claimed_before = claimed((interactive, general))

    # Act
    stopped = stack_ctl.call("process-kill", service="worker", op="stop")
    rows = new_rows(stack_ctl, rows_before)
    _, photo_done = wait_for_row(stack_ctl, lambda job, row: job in rows and row["kind"] == "stills"
                                 and row["status"] == "succeeded", seconds=5)

    # Assert: the status reports before the stop come every second, healthy, 20 days old, on the API's clock.
    assert [(r["decoder_ok"], r["image_age_days"]) for r in (first, second)] == [(1, BUILD_AGE_DAYS)] * 2, (first, second)
    assert second["seen_ms"] > first["seen_ms"] and waited <= STATUS_INTERVAL_SECONDS * 2, (first, second, waited)
    assert abs(second["seen_ms"] - host_ms) <= CLOCK_SLACK_MS, (second, host_ms)
    # The drain: exit 0 after the grace and before the stop's SIGKILL.
    assert stopped["exit_code"] == 0, stopped
    assert SHUTDOWN_GRACE_SECONDS - 0.5 <= stopped["seconds"] < EXIT_WITHIN_SECONDS, stopped
    # The held transcode is cancelled and given back; the running photo finished; the queued photo was never claimed.
    final = new_rows(stack_ctl, rows_before)
    assert requests_of(general.record(), "cancel"), general.record()
    assert [(r["status"], r["attempts"]) for r in rows.values() if r["kind"] == "transcode"] == [("queued", 0)], rows
    assert photo_done is not None, final
    assert sorted(r["status"] for r in final.values() if r["kind"] == "stills") == ["queued", "succeeded"], final
    assert claimed((interactive, general)) == claimed_before, (interactive.record(), general.record())
