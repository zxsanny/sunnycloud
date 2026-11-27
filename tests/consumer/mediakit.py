"""What the media suites read from the files they get back: the size of a WebP, and the facts of an MP4 (index position,
H.264 profile, picture size, AAC). Byte reading only: the consumer image carries no media tools, and a test that parsed
the file with the decoder's own libraries would share its blind spots."""
import hashlib
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


def _boxes(data, start, end):
    """`(kind, body start, end)` of the boxes between `start` and `end`."""
    offset = start
    while offset + 8 <= end:
        size, kind = struct.unpack(">I4s", data[offset:offset + 8])
        header = 8
        if size == 1:
            size, header = struct.unpack(">Q", data[offset + 8:offset + 16])[0], 16
        elif size == 0:
            size = end - offset
        assert size >= header, f"bad box size at {offset}"
        yield kind, offset + header, offset + size
        offset += size


def _child(data, span, kind):
    found = next(((start, end) for name, start, end in _boxes(data, *span) if name == kind), None)
    assert found, f"no {kind!r} box"
    return found


def _table(data, span, entry_format):
    """The entries of a full box that is a counted table (`stco`, `co64`, `stsc`)."""
    count = struct.unpack(">I", data[span[0] + 4:span[0] + 8])[0]
    size = struct.calcsize(entry_format)
    return [struct.unpack(entry_format, data[span[0] + 8 + i * size:span[0] + 8 + (i + 1) * size]) for i in range(count)]


def track_sample_hashes(data):
    """`{handler type: SHA-256 of the track's samples in decode order}` ("vide", "soun"): the coded packets, as ffprobe's
    packet hash compares them, read from the sample tables (`stsz`, `stsc`, `stco`/`co64`). A remux that only moves the
    index keeps every hash; a re-encode changes them."""
    hashes = {}
    for kind, start, end in _boxes(data, *_child(data, (0, len(data)), b"moov")):
        if kind != b"trak":
            continue
        mdia = _child(data, (start, end), b"mdia")
        hdlr = _child(data, mdia, b"hdlr")
        handler = data[hdlr[0] + 8:hdlr[0] + 12].decode("ascii")
        stbl = _child(data, _child(data, mdia, b"minf"), b"stbl")
        stsz = _child(data, stbl, b"stsz")
        fixed, count = struct.unpack(">II", data[stsz[0] + 4:stsz[0] + 12])
        sizes = [fixed] * count if fixed else list(struct.unpack(f">{count}I", data[stsz[0] + 12:stsz[0] + 12 + 4 * count]))
        names = {name for name, _, _ in _boxes(data, *stbl)}
        offsets = [o for (o,) in _table(data, _child(data, stbl, b"co64" if b"co64" in names else b"stco"),
                                        ">Q" if b"co64" in names else ">I")]
        runs = _table(data, _child(data, stbl, b"stsc"), ">III")
        digest, sample = hashlib.sha256(), 0
        for chunk, offset in enumerate(offsets, start=1):
            per_chunk = next(samples for first, samples, _ in reversed(runs) if first <= chunk)
            for _ in range(per_chunk):
                assert sample < count, f"{handler}: the chunks hold more samples than stsz's {count}"
                digest.update(data[offset:offset + sizes[sample]])
                offset += sizes[sample]
                sample += 1
        assert sample == count, f"{handler}: {sample} samples in chunks, {count} in stsz"
        assert handler not in hashes, f"two {handler} tracks"
        hashes[handler] = digest.hexdigest()
    assert hashes, "no track"
    return hashes
