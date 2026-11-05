# VideoExecutor: remux, CPU encode, scale, HDR to SDR, VFR/audio sync, output validation

**Task**: AZ-3860_decoder_video_executor
**Name**: Decoder VideoExecutor
**Description**: Execute `transcode` for `Plan::Remux` (lossless copy, index to the front) and `Plan::Transcode` (H.264 High 1080p AAC MP4 with faststart, orientation-independent scale, HDR tone map, VFR and audio sync), map only the first video and audio stream, and validate the output by probing it
**Complexity**: 5 points
**Dependencies**: AZ-3854_decoder_process_guard, AZ-3858_decoder_probe, AZ-3849_decoder_s5_whitelist_hdr_spike, AZ-3863_decoder_media_build_stages, AZ-3837_media_shared_params_protocol
**Component**: 10_decoder
**Tracker**: AZ-3860
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/10_decoder/decoder_protocol.md` — `transcode`, `TranscodeResult`, `Stalled`, `EncoderFailed`, `OutputInvalid`.
- `_docs/02_document/contracts/media-shared/skeleton_types.md` — `Plan`, `Probe`, `Params`.

## Problem

Phone clips arrive in many shapes: HEVC, HDR (HLG/PQ), rotated, variable frame rate, with GPS tags, subtitles and the index at the end. The decoder must produce one compliant output per plan (AC-11, AC-44) without re-encoding what only needs a remux, without leaking metadata, with correct orientation and audio sync (AC-45), and must report the output's real properties so the supervisor can apply the compliance check.

## Outcome

- `Plan::Remux` copies the first video and audio streams bit for bit and moves the index to the front.
- `Plan::Transcode` produces H.264 High `yuv420p` BT.709, AAC 128 kb/s stereo, faststart, short side at most 1080 and long side at most 1920.
- Every output is probed and returned as `probe_of_output`; an output that is not one H.264 stream of the input's duration is `OutputInvalid`.

## Scope

### Included
- Input options on every ffmpeg call: `-nostdin -protocol_whitelist file -format_whitelist <S5> -codec_whitelist <S5>`; `in/source` read as a file, never a pipe.
- Mapping: first video and first audio stream only; no subtitles, data or chapters; `-map_metadata -1`.
- Remux: `-c copy -movflags +faststart`, no filters.
- CPU encode with the values of `Params` (`libx264 -preset veryfast -crf 23 -profile:v high -pix_fmt yuv420p -maxrate 10M -bufsize 20M`, `-c:a aac -b:a 128k -ac 2`, `-movflags +faststart`, BT.709 tags); `-threads` and `-filter_threads` equal to the clamped cpu limit.
- Scale: short edge `min(1080, short)`, long edge at most 1920, even dimensions, orientation independent; ffmpeg's default autorotate kept (no `-noautorotate`).
- HDR (HLG/PQ) to SDR with the chain fixed by gate S5 (downscale first, `zscale` linear, `tonemap=hable`, `zscale` BT.709, `yuv420p`).
- VFR and audio: `-fps_mode vfr` and `aresample=async=1`.
- `Progress{out_time_ms, speed}` from each progress line; stall and wall handled by `ProcessGuard`.
- Output validation: ffprobe of `out/result.mp4`; duration within 1 s of the input and exactly one H.264 video stream, else `OutputInvalid`; the probe returned in `TranscodeResult`.

### Excluded
- Choosing the plan (E-20/c10a with `media-shared::policy`); the NVENC encoder (E-19/c13, v1.1); retry on CPU (E-20/c06).

## Acceptance Criteria

**AC-1: CPU transcode output (AC-11, AC-44, IT-10-8)**
Given DS-05 base (1080p, 18-20 Mbit/s), a 4K SDR clip, a portrait 2160x3840 clip and a clip with GPS/title metadata and a subtitle stream, with `Plan::Transcode{Cpu, scale_to_short_edge: 1080, tone_map: false, audio: aac, faststart: true}`
When each is transcoded
Then each output is H.264 High `yuv420p` with BT.709 tags, video bitrate at most 10 Mbit/s on average and in the worst 1 s window, AAC 128 kb/s stereo, `moov` before `mdat`; 1080p stays 1920x1080, 4K becomes 1920x1080, portrait becomes 1080x1920, all even; only one video and one audio stream, no subtitles and no GPS or title tags; duration within 1 s of the input; and the DS-05 output is smaller than its input

**AC-2: Reported probe is the real one**
Given any successful transcode
When `probe_of_output` is compared with an independent ffprobe of `out/result.mp4`
Then they are equal

**AC-3: Lossless remux (IT-10-11)**
Given the DS-35 moov-at-end clip and a variant with an extra subtitle stream and chapters, with `Plan::Remux`
When each is remuxed
Then the MD5 of the video and audio packet payloads equals the input's, codec, profile, bitrate and frame count are unchanged, `moov` is before `mdat`, subtitles, data streams, chapters and global metadata are dropped, `probe_of_output.moov_first` is true, and wall time is at most 20% of a transcode of the same clip

**AC-4: HDR to SDR (IT-10-10)**
Given DS-06 base (4K HEVC Main10 HLG) with `tone_map: true`
When it is transcoded
Then the output is 1920x1080 `yuv420p` with `bt709` primaries, transfer and matrix, H.264 High, and its mean luma lies within the tolerance fixed by gate S5 (recorded until S5 fixes it)

**AC-5: Orientation and sync (AC-45, IT-10-9)**
Given three DS-08 clips rotated by display matrix 90/180/270 and three VFR clips with beeps at known times
When each is transcoded
Then the displayed orientation matches the flash-marker position of the manifest with no rotation tag left to double-rotate, and every beep onset is within 40 ms of the manifest time

**AC-6: Invalid output refused**
Given an encode whose output has no video stream or a duration 2 s shorter than the input (stub variant)
When validation runs
Then the reply is `OutputInvalid`

**AC-7: Argument hygiene (ST-10-2, IT-10-12)**
Given a photo-sized and a video job with a cpu limit clamped to 4
When the argv log is read
Then every ffmpeg and ffprobe call has `-nostdin`, `-protocol_whitelist file`, `-format_whitelist`, `-codec_whitelist`, `-map_metadata -1`, first-stream mapping only, `-threads 4` and `-filter_threads 4`

## Non-Functional Requirements

**Performance**
- 1080p transcode at most 0.5 s per second of media; 4K HLG at most 2.0 s/s; output/input ratio on DS-05 at most 20% (failure 35%) (PT-10-2 on REF-CPU4, measured in E-19/c16; full budgets NFT-PERF-15/16).

**Reliability**
- A stalled encode is killed within 90 s of its last progress line (AC-45, via `ProcessGuard`).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1, AC-3, AC-4, AC-7 | Argv builders for remux, CPU encode, HDR, VFR (golden files) | Exact argv per plan |
| AC-1 | Scale computation for landscape, portrait, 4K and small inputs | Short <= 1080, long <= 1920, even |
| AC-6 | Output validation from recorded probes | `OutputInvalid` for each failing property |

## Blackbox Tests

Runner-level cases run against the runner in the decoder image (E-19/c14a); the production-container run is E-19/c15.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | DS-05 base, 4K, portrait, metadata clip | IT-10-8 | Compliant 1080p outputs; probe equals ffprobe | AC-11, AC-44, FT-P-24, FT-P-26 |
| AC-3 | DS-35 moov-at-end and variant | IT-10-11 | Packets identical, index first | AC-11, FT-P-25 |
| AC-4 | DS-06 base | IT-10-10 | SDR BT.709 1080p | AC-11, NFT-PERF-16 |
| AC-5 | DS-08 rotated and VFR | IT-10-9 | Orientation right, drift <= 40 ms | AC-45, FT-P-27 |

## Runtime Completeness

- Named capability: ffmpeg 8.x/9.x with libx264, native `aac` and libzimg (`zscale`, `tonemap`), built by E-19/c14a.
- Production code that must exist: real ffmpeg/ffprobe invocations for remux, CPU encode, HDR tone map and output validation.
- Allowed external stubs: the `test-stubs` variant for AC-6 only.
- Unacceptable substitutes: copying the input as the "transcode", skipping validation, or a passthrough that returns the input probe.

## Constraints

- The decoder never receives free-form ffmpeg arguments; argv comes only from the `Plan` enumeration and `Params`.
- GPL image with native `aac`; no `libfdk_aac`, no non-free filters (ADR-011).

## Risks & Mitigation

**Risk 1: 4K HLG misses 2.0 s/s**
- *Risk*: tone mapping before scaling is too slow on 4 cores.
- *Mitigation*: downscale first (S5 chain); measured in E-19/c01 and E-19/c16.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| CRF, preset and maxrate must come from `Params[params_version]`, but `Plan` carries no version (decoder protocol contract open point 7) | E-18 and E-00/c06 owners | open | Medium |
| The error for a clean non-zero exit of the CPU encoder is not stated (`EncoderFailed` is described for NVENC), nor what follows a second `EncoderFailed` after the CPU rerun (09 §3 lists only a second `Stalled` as permanent) | E-19 and E-20 owners | open | Low |
