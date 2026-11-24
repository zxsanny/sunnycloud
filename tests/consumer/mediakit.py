"""What the media suites read from the files they get back: the size of a WebP, and the facts of an MP4 (index position,
H.264 profile, picture size, AAC). Byte reading only: the consumer image carries no media tools, and a test that parsed
the file with the decoder's own libraries would share its blind spots."""
import struct

FREE_BOX = b"free"


def webp_size(data):
    """`(width, height)` of a WebP (lossy, lossless or extended)."""
    assert data[:4] == b"RIFF" and data[8:12] == b"WEBP", f"not a WebP: {data[:12]!r}"
    kind, body = data[12:16], data[20:]
    if kind == b"VP8 ":
        assert body[3:6] == b"\x9d\x01\x2a", "no VP8 start code"
        return struct.unpack("<HH", body[6:10])[0] & 0x3FFF, struct.unpack("<HH", body[6:10])[1] & 0x3FFF
    if kind == b"VP8L":
        bits = struct.unpack("<I", body[1:5])[0]
        return (bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1
    if kind == b"VP8X":
        return int.from_bytes(body[4:7], "little") + 1, int.from_bytes(body[7:10], "little") + 1
    raise AssertionError(f"unknown WebP chunk {kind!r}")


def top_level_boxes(data):
    boxes, offset = [], 0
    while offset + 8 <= len(data):
        size, kind = struct.unpack(">I4s", data[offset:offset + 8])
        if size == 1:
            size = struct.unpack(">Q", data[offset + 8:offset + 16])[0]
        elif size == 0:
            size = len(data) - offset
        assert size >= 8, f"bad box size at {offset}"
        boxes.append(kind)
        offset += size
    return boxes


def mp4_facts(data):
    boxes = top_level_boxes(data)
    facts = {"brand": data[8:12], "moov_first": boxes.index(b"moov") < boxes.index(b"mdat"), "aac": b"mp4a" in data}
    # `avc1` also appears in the `ftyp` brand list, so the sample entry is looked for after the first `stsd` box.
    sample_table = data.find(b"stsd")
    avc = data.find(b"avc1", sample_table) if sample_table >= 0 else -1
    config = data.find(b"avcC", avc) if avc >= 0 else -1
    facts["h264"] = avc >= 0 and config >= 0
    if facts["h264"]:
        facts["width"], facts["height"] = struct.unpack(">HH", data[avc + 4 + 24:avc + 4 + 28])
        facts["profile_idc"] = data[config + 5]  # 66 baseline, 77 main, 100 high
    return facts


def with_marker(data, marker):
    """The same file with a unique tail, so that it has another content hash (a second upload is then a new job). An
    MP4 gets a `free` box after the last box; a JPEG gets bytes after its end marker, which decoders ignore."""
    marker = marker.encode()
    if data[:2] == b"\xff\xd8":
        return data + marker
    return data + struct.pack(">I4s", 8 + len(marker), FREE_BOX) + marker


def mdat_size(data):
    """The size of the `mdat` box: the coded audio and video. A lossless remux keeps it; a re-encode changes it."""
    offset = 0
    while offset + 8 <= len(data):
        size, kind = struct.unpack(">I4s", data[offset:offset + 8])
        if kind == b"mdat":
            return size
        offset += size
    raise AssertionError("no mdat box")
