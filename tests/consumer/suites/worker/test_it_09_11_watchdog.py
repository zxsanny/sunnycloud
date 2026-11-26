"""IT-09-11 (AC-45) on H2: the supervisor's watchdog. A decoder that goes silent is cancelled after
`decoder_silence_timeout` (1 s); one that keeps sending `Alive` but never answers a photo render is cancelled at
`photo_job_timeout` (5 s on H2, the case's 2 s scaled to the configured value); either way the job is not lost and the
slot is gated again before it takes another job. A `Stalled` answer is handled as in IT-09-6."""
import time

from h2cases import PHOTO, photo_probe, postable_image, requests_of, run_file

SILENCE_MS = 1000
PHOTO_TIMEOUT_MS = 5000
CANCEL_SLACK_MS = 600
EVENT_SECONDS = 20
POLL_SECONDS = 0.25


def wait_record(fd, done, seconds=EVENT_SECONDS):
    deadline = time.monotonic() + seconds
    while True:
        record = fd.record()
        if done(record) or time.monotonic() > deadline:
            return record
        time.sleep(POLL_SECONDS)


def gated_again_after(record, at_ms):
    """The kinds FD saw on connections opened after `at_ms`."""
    later = {line["conn"] for line in record if line["conn"] and line["at_ms"] > at_ms and line["kind"] == "hello"}
    return [line["kind"] for line in record if line["conn"] in later]


def test_it_09_11_a_silent_decoder_is_cancelled_after_one_second_and_the_slot_is_gated_again(media, lanes):
    # Act
    fd, _, before, _ = run_file(media, lanes, "it-09-11-silent", {"probe": {"queue": ["silent"],
                                "default": {"probe": photo_probe()}}, "render_still": {"default": postable_image()}},
                                fixture=PHOTO, wanted=lambda before, after: True)
    record = wait_record(fd, lambda r: requests_of(r, "cancel") and "hello" in gated_again_after(r, requests_of(r, "cancel")[0]["at_ms"]))
    after = media.jobs()

    # Assert
    probe, cancel = requests_of(record, "probe")[0], requests_of(record, "cancel")[0]
    assert SILENCE_MS - 100 <= cancel["at_ms"] - probe["at_ms"] <= SILENCE_MS + CANCEL_SLACK_MS, (probe, cancel)
    assert gated_again_after(record, cancel["at_ms"])[:2] == ["hello", "self_check"], record
    assert after["dead"] == before["dead"], "a silent decoder is a transient failure, never a dead job"


def test_it_09_11_a_render_that_only_sends_alive_is_cancelled_at_the_photo_timeout(media, lanes):
    # Act
    fd, _, before, _ = run_file(media, lanes, "it-09-11-wall", {"probe": {"default": {"probe": photo_probe()}},
                                "render_still": {"queue": [{"alive_only": {"every_ms": 200}}], "default": postable_image()}},
                                fixture=PHOTO, wanted=lambda before, after: True)
    record = wait_record(fd, lambda r: requests_of(r, "cancel"))
    after = media.jobs()

    # Assert
    render, cancel = requests_of(record, "render_still")[0], requests_of(record, "cancel")[0]
    assert PHOTO_TIMEOUT_MS - 200 <= cancel["at_ms"] - render["at_ms"] <= PHOTO_TIMEOUT_MS + CANCEL_SLACK_MS, (render, cancel)
    assert after["dead"] == before["dead"]


def test_it_09_11_a_stalled_render_is_rerun_once_and_the_job_succeeds(media, lanes):
    # Act
    _, record, before, after = run_file(media, lanes, "it-09-11-stalled", {"probe": {"default": {"probe": photo_probe()}},
                                        "render_still": {"queue": [{"error": "stalled"}], "default": postable_image()}},
                                        fixture=PHOTO, ended=1)

    # Assert
    assert len(requests_of(record, "render_still")) == 3, record
    assert after["derived"] - before["derived"] == 1, (before, after)
