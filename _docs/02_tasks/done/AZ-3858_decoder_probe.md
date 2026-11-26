# probe for photos (header read) and videos (ffprobe parse, moov_first scan)

**Task**: AZ-3858_decoder_probe
**Name**: Decoder probe
**Description**: Answer `probe` with facts only: photo width and height from a header read, and for video the ffprobe streams and format parsed into `Probe` plus `moov_first` from a bounded scan of top-level MP4 boxes
**Complexity**: 3 points
**Dependencies**: AZ-3857_decoder_image_executor, AZ-3854_decoder_process_guard, AZ-3856_decoder_safe_open_watchdog, AZ-3849_decoder_s5_whitelist_hdr_spike, AZ-3863_decoder_media_build_stages
**Component**: 10_decoder
**Tracker**: AZ-3858
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/10_decoder/decoder_protocol.md` — `probe` request and errors.
- `_docs/02_document/contracts/media-shared/skeleton_types.md` — `Probe` fields.

## Problem

The supervisor probes every job, photos included, and decides the transcode policy from the answer (09 §3, 09 risk 5). The probe must not decode a photo (that would add latency to AC-10), must report video facts precisely enough for the compliance rule (codec, profile, size, rotation, bitrate, colour, audio, index position), and must never judge compliance itself.

## Outcome

- Photo probe: `Probe{kind: photo, width, height}` from the header, fast and with no decode.
- Video probe: every `Probe` field filled from ffprobe run with the whitelists; `moov_first` from at most 64 box headers.
- Probe failures are typed (`Unsupported`, `Corrupt`, `LimitExceeded`).

## Scope

### Included
- Photos: `vipsheader` under the profile chosen by sniffing (E-19/c08); width and height only.
- Videos: `ffprobe -v error -show_streams -show_format -of json` with `-nostdin -protocol_whitelist file -format_whitelist -codec_whitelist` (lists from gate S5), reading `in/source` as a file; parsed into codec, profile, width, height, rotation, bitrate, pix_fmt, primaries, transfer, matrix, audio codec, duration_ms, container.
- `moov_first`: scan of top-level MP4 box headers, header bytes only, at most 64 boxes.
- Exit codes and parse failures mapped to typed errors; ffprobe output parsed with a size bound.

### Excluded
- Any compliance judgement (`media-shared::policy`, applied by E-20); probing outputs after a transcode (E-19/c11 output validation).

## Acceptance Criteria

**AC-1: Photo probe is a header read (IT-10-4)**
Given DS-03 JPEG 12 MP, HEIC 12 MP, a PNG, a WebP, a GIF and the DS-16 30000x30000 PNG
When each is probed
Then width and height equal the manifest, each 12 MP probe takes at most 50 ms and the huge PNG at most 100 ms, and runner memory grows by less than 50 MiB

**AC-2: Truncated photo**
Given a truncated JPEG
When it is probed
Then the answer is `Corrupt`, or valid dimensions when the header is intact, and the outcome is recorded by the test

**AC-3: Video facts (AC-11)**
Given DS-07 compliant clips, a DS-35 moov-at-end clip, a 4K HLG clip and a rotated DS-08 clip
When each is probed
Then the `Probe` fields equal an independent ffprobe of the same file, `transfer` shows HLG for the HDR clip, and `rotation` shows the display matrix angle for the rotated clip

**AC-4: `moov_first` (IT-10-11)**
Given a clip with `moov` before `mdat`, one with `moov` at the end, and a file with more than 64 top-level boxes
When each is probed
Then the first is `moov_first = true`, the second `false`, and the scan reads at most 64 box headers in the third

**AC-5: Hostile video input (AC-25)**
Given an `.m3u8` naming `file:///etc/passwd` saved as `.mp4` and a concat file
When each is probed
Then each is a typed failure and no byte of `/etc/passwd` appears in any reply or log

## Non-Functional Requirements

**Performance**
- Photo probe p95 30 ms (failure 100 ms) (PT-10-1, measured in E-19/c16).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | ffprobe JSON parsing from recorded outputs (photo and video) | Fields mapped; unknown values typed |
| AC-4 | Box scanner on crafted headers (moov first, last, 65 boxes, truncated header) | Correct flag; at most 64 headers read |
| AC-5 | Argv builder (golden) | Whitelist options present on every call |

## Blackbox Tests

Runner-level cases run against the runner in the decoder image (E-19/c14a); the production-container run is E-19/c15.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | DS-03, DS-16 huge PNG, truncated JPEG | IT-10-4 | Manifest dimensions; latency and memory bounds | AC-10, PT-10-1 |
| AC-3, AC-4 | DS-07, DS-35, DS-06 base, DS-08 | Probe vs independent ffprobe | Equal facts | AC-11, FT-P-25 |
| AC-5 | DS-16 playlist and concat items | IT-10-20 probe part | Typed failure, no disclosure | NFT-SEC-28, NFT-SEC-29 |

## Runtime Completeness

- Named capability: `vipsheader` and `ffprobe` from the decoder image (E-19/c14a).
- Production code that must exist: real invocation under `ProcessGuard` and the parser of their output.
- Unacceptable substitutes: reading dimensions or streams from file names, extensions or a fixed table.

## Constraints

- The decoder reports facts and never calls `media-shared::policy` (10 §1).

## Risks & Mitigation

**Risk 1: Photo probe adds latency**
- *Risk*: a header read that triggers a decode costs hundreds of milliseconds per photo.
- *Mitigation*: AC-1 bounds time and memory for 12 MP and 900 MP inputs.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The result when no `moov` is found within the first 64 boxes is not stated (assumed `moov_first = false`, the safe value for the remux rule) | E-19 owner | open | Low |
