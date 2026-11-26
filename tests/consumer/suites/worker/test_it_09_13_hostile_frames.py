"""IT-09-13 (AC-71) on H2: hostile frames from the decoder. Each one is a protocol violation of that decoder: the job
fails transiently, the slot reconnects and repeats `hello` and `self_check` before it claims again, and the supervisor
process stays up without growing (resident memory under 10 MiB more)."""
import json
import struct
import time

import pytest

from h2cases import PHOTO, requests_of, run_file

EVENT_SECONDS = 20
POLL_SECONDS = 0.25
MAX_GROWTH = 10 * 1024 * 1024
RSS = "sunnycloud_process_resident_memory_bytes"


def probe_reply(width, duration_ms, kind):
    return {"response": {"probe": {
        "kind": kind, "container": "mp4", "moov_first": True,
        "video": {"codec": "h264", "profile": None, "width": width, "height": 1080, "rotation": None, "bitrate": None,
                  "pix_fmt": None, "primaries": None, "transfer": None, "matrix": None},
        "audio": None, "duration_ms": duration_ms}}}


def framed(payload):
    return struct.pack(">I", len(payload)) + payload


def json_frame(value):
    return framed(json.dumps(value).encode())


def extra_field():
    reply = probe_reply(1920, 1000, "video")
    reply["response"]["probe"]["surprise"] = 1
    return reply


FRAMES = {
    "oversize-64kib-plus-1": struct.pack(">I", 65_537) + b" " * 65_537,
    "unknown-field": json_frame(extra_field()),
    "width-0": json_frame(probe_reply(0, 1000, "video")),
    "width-4294967295": json_frame(probe_reply(4_294_967_295, 1000, "video")),
    "negative-duration": json_frame(probe_reply(1920, -1, "video")),
    "unknown-media-kind": json_frame(probe_reply(1920, 1000, "audio")),
    "invalid-utf8": framed(b"{\xff\xfe}"),
    "announced-length-then-stall": struct.pack(">I", 100) + b'{"response"',
}


@pytest.mark.parametrize("case", sorted(FRAMES))
def test_it_09_13_a_hostile_frame_fails_the_job_transiently_and_the_slot_is_gated_again(stack_ctl, media, lanes, case):
    # Arrange
    script = {"probe": {"default": {"garbage": {"framed": False, "chunks": [{"hex": FRAMES[case].hex()}]}}}}

    # Act
    fd, _, before, _ = run_file(media, lanes, f"it-09-13-{case}", script, fixture=PHOTO,
                                wanted=lambda before, after: True)
    started = stack_ctl.call("inspect", service="worker")["started_at"]
    rss_before = stack_ctl.worker_metrics()[RSS]
    deadline = time.monotonic() + EVENT_SECONDS
    while time.monotonic() < deadline:
        record = fd.record()
        probes = requests_of(record, "probe")
        if probes and [line["kind"] for line in record if line["conn"] > probes[0]["conn"]][:2] == ["hello", "self_check"]:
            break
        time.sleep(POLL_SECONDS)
    after = media.jobs()
    rss_after = stack_ctl.worker_metrics()[RSS]
    worker = stack_ctl.call("inspect", service="worker")

    # Assert
    probes = requests_of(record, "probe")
    reconnected = [line["kind"] for line in record if probes and line["conn"] > probes[0]["conn"]]
    assert probes and reconnected[:2] == ["hello", "self_check"], record
    assert after["dead"] == before["dead"] and after["derived"] == before["derived"], (before, after)
    assert (worker["status"], worker["started_at"]) == ("running", started)
    assert rss_after - rss_before < MAX_GROWTH, (rss_before, rss_after)
