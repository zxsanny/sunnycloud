"""What the worker cases on H2 share: probe answers in the protocol's form, outputs that pass the API's sniffer, and
one file run through FD on the general lane while the interactive slot is gated out (so every job of the file goes to
that one FD, in order).

FD's outputs are protocol traffic: a 1x1 WebP and a bare MP4 `ftyp` box pass the API's sniffer
(`components/08_media_jobs/src/sniff.rs` reads the first 32 bytes) and claim no media outcome."""
import base64
import time

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


def terminal(jobs):
    return sum(jobs[key] for key in ("derived", "skipped_compliant", "obsolete", "dead"))


def run_file(media, lanes, name, fd_script, fixture=CLIP, ended=2, wanted=None):
    """Gates the interactive slot out, puts FD with `fd_script` on the general lane, commits one file and waits until
    `ended` of its jobs reached a terminal state, or until `wanted(before, after)` holds when given; returns FD's lane,
    its record and the job counts before and after."""
    lanes("interactive", {"self_check": FAILED_SELF_CHECK}, restart_worker=False)
    fd = lanes("general", fd_script)
    fd.wait_for(["hello", "self_check"], GATE_SECONDS)
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
