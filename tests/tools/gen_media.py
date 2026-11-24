#!/usr/bin/env python3
"""Small media fixtures for the media-pipeline suites (AZ-3661), made with the host's ffmpeg.

    gen_media.py <out-dir>

Not the DS-03..DS-08 corpora of `test-data.md` (those are large and E-31's): a second or two of synthetic video (each file at most 2 MiB, so one small upload carries it), one large
JPEG with EXIF orientation 6 and two hostile files. The decoder image's own ffmpeg is a minimal build without synthetic sources, so the host's is used. Each clip is checked with ffprobe, so a fixture
that is not what its name says fails here and not as a puzzling policy result in a suite. Python stdlib, ffmpeg and ffprobe only.
"""
import argparse
import json
import struct
import subprocess
import sys
from pathlib import Path

TIMEOUT_SECONDS = 180


def run_tool(tool, args):
    done = subprocess.run([tool, *args], stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=TIMEOUT_SECONDS, check=False)
    if done.returncode != 0:
        sys.exit(f"{tool} {' '.join(args)}\nexit {done.returncode}\n{done.stderr[-1500:]}")
    return done.stdout


def probe(path):
    text = run_tool("ffprobe", ["-v", "error", "-print_format", "json", "-show_format", "-show_streams", str(path)])
    return json.loads(text)


def video_args(size, rate, bitrate, seconds, extra=()):
    return ["-f", "lavfi", "-i", f"testsrc2=size={size}:rate={rate}:duration={seconds},noise=alls=30:allf=t",
            "-f", "lavfi", "-i", f"sine=frequency=440:duration={seconds}",
            "-c:v", "libx264", "-profile:v", "high", "-pix_fmt", "yuv420p", "-b:v", bitrate, "-maxrate", bitrate,
            "-bufsize", "4M", "-x264-params", "colorprim=bt709:transfer=bt709:colormatrix=bt709",
            "-c:a", "aac", "-b:a", "128k", "-shortest", *extra]


def moov_before_mdat(path):
    """True when the `moov` box comes before `mdat` (an index at the front: the file plays while it downloads)."""
    seen = []
    with open(path, "rb") as handle:
        while True:
            head = handle.read(8)
            if len(head) < 8:
                break
            size, kind = struct.unpack(">I4s", head)
            seen.append(kind)
            if size == 1:
                size = struct.unpack(">Q", handle.read(8))[0]
                handle.seek(size - 16, 1)
            elif size == 0:
                break
            else:
                handle.seek(size - 8, 1)
    return seen.index(b"moov") < seen.index(b"mdat")


def exif_orientation(jpeg, orientation):
    """The JPEG with an APP1 Exif segment that holds one tag: Orientation."""
    tiff = b"MM\x00\x2a\x00\x00\x00\x08" + struct.pack(">H", 1) + struct.pack(">HHI", 0x0112, 3, 1) + struct.pack(">HH", orientation, 0) + b"\x00\x00\x00\x00"
    payload = b"Exif\x00\x00" + tiff
    segment = b"\xff\xe1" + struct.pack(">H", len(payload) + 2) + payload
    assert jpeg[:2] == b"\xff\xd8"
    return jpeg[:2] + segment + jpeg[2:]


def truncated_heic():
    ftyp = struct.pack(">I4s4sI4s4s", 24, b"ftyp", b"heic", 0, b"mif1", b"heic")
    meta_head = struct.pack(">I4s", 4000, b"meta") + b"\x00\x00\x00\x00" + struct.pack(">I4s", 40, b"hdlr") + b"\x00" * 8
    return ftyp + meta_head


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("out_dir")
    args = parser.parse_args()
    out = Path(args.out_dir).resolve()
    out.mkdir(parents=True, exist_ok=True)

    run_tool("ffmpeg", ["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=2400x1800", "-frames:v", "1", "-q:v", "3", str(out) + "/_plain.jpg"])
    (out / "photo-orientation6.jpg").write_bytes(exif_orientation((out / "_plain.jpg").read_bytes(), 6))
    (out / "_plain.jpg").unlink()

    run_tool("ffmpeg", ["-y", "-v", "error", *video_args("1920x1080", 30, "12M", 1), str(out) + "/clip-noncompliant.mp4"])
    front = ["-movflags", "+faststart"]
    run_tool("ffmpeg", ["-y", "-v", "error", *video_args("1280x720", 30, "4M", 2, front), str(out) + "/clip-compliant-front.mp4"])
    run_tool("ffmpeg", ["-y", "-v", "error", *video_args("1280x720", 30, "4M", 2), str(out) + "/clip-compliant-end.mp4"])

    (out / "hostile-svg.png").write_text('<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><script>alert(1)</script></svg>\n', encoding="utf-8")
    (out / "truncated.heic").write_bytes(truncated_heic())

    noncompliant = probe(out / "clip-noncompliant.mp4")
    rate = int(noncompliant["format"]["bit_rate"])
    assert (out / "clip-noncompliant.mp4").stat().st_size <= 2 * 1024 * 1024, "a clip must fit one 2 MiB block"
    assert rate > 10_000_000, f"clip-noncompliant.mp4 must be above 10 Mbit/s, is {rate}"
    for name, front_index in (("clip-compliant-front.mp4", True), ("clip-compliant-end.mp4", False)):
        info = probe(out / name)
        video = next(s for s in info["streams"] if s["codec_type"] == "video")
        assert video["codec_name"] == "h264" and video["profile"] == "High", (name, video["profile"])
        assert int(info["format"]["bit_rate"]) <= 10_000_000, (name, info["format"]["bit_rate"])
        tags = (video.get("color_primaries"), video.get("color_transfer"), video.get("color_space"))
        assert tags == ("bt709",) * 3, f"{name}: colour tags {tags}"
        assert moov_before_mdat(out / name) is front_index, f"{name}: index position is not as named"
    print(f"media fixtures in {out}: " + ", ".join(sorted(p.name for p in out.iterdir())))


if __name__ == "__main__":
    main()
