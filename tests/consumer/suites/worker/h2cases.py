"""What the worker cases on H2 share: probe answers in the protocol's form, outputs that pass the API's sniffer, and
one file run through FD on the general lane while the interactive slot is gated out (so every job of the file goes to
that one FD, in order).

FD's outputs are protocol traffic: a 1x1 WebP and a bare MP4 `ftyp` box pass the API's sniffer
(`components/08_media_jobs/src/sniff.rs` reads the first 32 bytes) and claim no media outcome."""
import base64
import json
import time
from datetime import datetime

import mediakit
from fdkit import FAILED_SELF_CHECK

GATE_SECONDS = 30
JOBS_SECONDS = 60
POLL_SECONDS = 0.25
CLIP = "clip-compliant-front.mp4"
PHOTO = "photo-orientation6.jpg"
WEBP_1X1 = base64.b64decode("UklGRhoAAABXRUJQVlA4TA0AAAAvAAAAEAcQERGIiP4HAA==")
MP4_FTYP = b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2avc1mp41" + b"\x00\x00\x00\x08free"


def video_stream(width, height, bitrate):
    return {"codec": "h264", "profile": "high", "width": width, "height": height, "rotation": None, "bitrate": bitrate,
            "pix_fmt": "yuv420p", "primaries": "bt709", "transfer": "bt709", "matrix": "bt709"}


def video_probe(duration_ms=60_000, width=1920, height=1080, bitrate=5_000_000, moov_first=True):
    """A clip every compliance axis holds for, unless an argument says otherwise (the worker tests' probes)."""
    return {"kind": "video", "container": "mov,mp4,m4a,3gp,3g2,mj2", "moov_first": moov_first,
            "video": video_stream(width, height, bitrate), "audio": {"codec": "aac"}, "duration_ms": duration_ms}


def photo_probe(width=4000, height=3000):
    return {"kind": "photo", "container": "jpeg", "moov_first": False,
            "video": {"codec": None, "profile": None, "width": width, "height": height, "rotation": None, "bitrate": None,
                      "pix_fmt": None, "primaries": None, "transfer": None, "matrix": None},
            "audio": None, "duration_ms": None}


def chunks(data):
    return {"chunks": [{"hex": data.hex()}]}


def postable_image():
    return {"image": {"width": 1, "height": 1, "output": {"bytes": chunks(WEBP_1X1)}}}


def postable_transcode():
    return {"transcode": {"probe_of_output": video_probe(), "output": {"bytes": chunks(MP4_FTYP)}}}


# A photo FD probes as one and renders twice into postable outputs: the job closes `derived`.
CLEAN_PHOTO = {"probe": {"default": {"probe": photo_probe()}}, "render_still": {"default": postable_image()}}


def terminal(jobs):
    return sum(jobs[key] for key in ("derived", "skipped_compliant", "obsolete", "dead"))


def fd_on_general(lanes, fd_script):
    """The interactive slot gated out and FD with `fd_script` on the general lane, past its gate: every job goes to
    that one FD, in order."""
    lanes("interactive", {"self_check": FAILED_SELF_CHECK}, restart_worker=False)
    fd = lanes("general", fd_script)
    fd.wait_for(["hello", "self_check"], GATE_SECONDS)
    return fd


def run_file(media, lanes, name, fd_script, fixture=CLIP, ended=2, wanted=None):
    """`fd_on_general`, then commits one file and waits until `ended` of its jobs reached a terminal state, or until
    `wanted(before, after)` holds when given; returns FD's lane, its record and the job counts before and after."""
    fd = fd_on_general(lanes, fd_script)
    before = media.jobs()
    suffix = ".mp4" if fixture.endswith(".mp4") else ".jpg"
    media.commit(f"{name}{suffix}", mediakit.with_marker(media.fixture(fixture), name))
    done = (lambda jobs: wanted(before, jobs)) if wanted else (lambda jobs: terminal(jobs) - terminal(before) >= ended)
    deadline = time.monotonic() + JOBS_SECONDS
    while True:
        after = media.jobs()
        if done(after) or time.monotonic() > deadline:
            return fd, fd.record(), before, after
        time.sleep(POLL_SECONDS)


def requests_of(record, kind):
    return [line for line in record if line["kind"] == kind]


# A clip whose transcode FD holds for HOLD_MS, sending `Alive` meanwhile (the silence watchdog stays quiet), every
# attempt alike; its poster is posted at once. A transcode, not a render: H2's photo timeout is 5 s, the video one 60 s.
HOLD_MS = 12_000
HELD_TRANSCODE = {"probe": {"default": {"probe": video_probe(bitrate=12_000_000)}},
                  "render_still": {"default": postable_image()},
                  "transcode": {"default": {"over_time": {"delay_ms": HOLD_MS, "alive_every_ms": 500,
                                                          "answer": postable_transcode()}}}}


def wait_for_request(fd, kind, seconds=JOBS_SECONDS):
    """FD's requests of `kind` once there is one, or the empty list after `seconds`."""
    deadline = time.monotonic() + seconds
    while not requests_of(fd.record(), kind) and time.monotonic() < deadline:
        time.sleep(POLL_SECONDS)
    return requests_of(fd.record(), kind)


def readiness_item(ctl, name):
    """The status of one `/readyz` item ("ok", "failing"), or None while the worker has no listener. With the
    interactive slot gated out (`run_file`) the overall status is 503 throughout, so a case reads the item it means."""
    status, body = ctl.worker_ops("/readyz")
    if not status:
        return None
    return {check["name"]: check["status"] for check in json.loads(body)["checks"]}.get(name)


# The worker input route as the api logs it: one "request answered" line, with this template, per non-2xx answer.
INPUT_ROUTE = "/api/v1/worker-jobs/{job_id}/input"


def job_rows(ctl):
    """Every job in creation order: `{job id: {"kind", "status", "attempts", "last_error", "blob"}}` (`last_error` ""
    before any failure). A worker's failure code is free text, so it is what lies between the fourth and the last
    colon."""
    rows = {}
    for value in ctl.call("db-probe", query="job_rows")["values"]:
        job, kind, status, attempts, rest = value.split(":", 4)
        last_error, _, blob = rest.rpartition(":")
        rows[job] = {"kind": kind, "status": status, "attempts": int(attempts), "last_error": last_error, "blob": blob}
    return rows


def new_rows(ctl, before):
    """The rows of the jobs that are not in `before` (a `job_rows` of earlier), in creation order."""
    return {job: row for job, row in job_rows(ctl).items() if job not in before}


def wait_for_row(ctl, wanted, seconds=JOBS_SECONDS):
    """The first `(job id, row)` of `job_rows` for which `wanted(job id, row)` holds, or `(None, None)` after
    `seconds`."""
    deadline = time.monotonic() + seconds
    while True:
        found = next(((job, row) for job, row in job_rows(ctl).items() if wanted(job, row)), None)
        if found or time.monotonic() > deadline:
            return found or (None, None)
        time.sleep(POLL_SECONDS)


def input_errors(ctl, after=0.0):
    """The api's non-2xx answers on the worker input route logged after `after` (seconds since the epoch), oldest first:
    `(time, status)`. ctl returns the newest 4 KiB of matching lines and the api log lives as long as its container, so
    a case marks the time of the newest answer before it acts instead of counting lines."""
    tail = ctl.call("inspect", logs="api", lines_with=INPUT_ROUTE)["log_tail"]
    lines = [json.loads(line) for line in tail.splitlines() if line.startswith("{")]
    answers = [(datetime.fromisoformat(line["ts"]).timestamp(), line["status"]) for line in lines
               if line.get("msg") == "request answered"]
    return [answer for answer in answers if answer[0] > after]


def gate_out_both_lanes(ctl, lanes):
    """Both lanes on FD failing `self_check`: the supervisor claims nothing, so a job waits `queued`."""
    lanes("interactive", {"self_check": FAILED_SELF_CHECK}, restart_worker=False)
    lanes("general", {"self_check": FAILED_SELF_CHECK})
    assert ctl.wait_worker_readyz(503, GATE_SECONDS) == 503


def settled_scratch(ctl, job, lane="general", seconds=JOBS_SECONDS):
    """The lane's scratch entries, listed while `job` is not running and its row stays the same around the listing,
    so the folder of an attempt in flight is not taken for one left behind."""
    deadline = time.monotonic() + seconds
    while True:
        before = job_rows(ctl)[job]
        entries = ctl.call("inspect", scratch=lane)["entries"]
        if (before["status"] != "running" and job_rows(ctl)[job] == before) or time.monotonic() > deadline:
            return entries
        time.sleep(POLL_SECONDS)
