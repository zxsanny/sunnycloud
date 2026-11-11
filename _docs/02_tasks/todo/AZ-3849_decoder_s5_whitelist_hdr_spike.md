# Gate S5: ffmpeg whitelists, progress sink and HDR tone-map order

**Task**: AZ-3849_decoder_s5_whitelist_hdr_spike
**Name**: Decoder gate S5 spike
**Description**: Fix the ffmpeg format and codec whitelists against phone clips and the red-team set, prove the progress sink works under `-protocol_whitelist file`, and choose the HDR tone-map order within the 4K HLG budget
**Complexity**: 3 points
**Dependencies**: AZ-3652_initial_structure; notes: E-31/c20 (test epic E-31, decomposed at greenfield Step 9); E-31/c21 (test epic E-31, decomposed at greenfield Step 9); E-31/c01 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 10_decoder
**Tracker**: AZ-3849
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c01

## Problem

The decoder's ffmpeg calls run with whitelists that decide both what users can upload and what an attacker can reach (AC-25). Lists that are too tight lose real phone clips; lists that are too loose weaken containment (10 risk 4). The stall watchdog depends on progress lines, but the progress sink may itself be blocked by `-protocol_whitelist file` (10 risk 5, E-19 risk 7). The HDR chain order and operator are only a proposal, and 10 minutes of 4K HLG must transcode in 20 minutes on the reference worker (AC-11).

## Outcome

- Final demuxer and decoder whitelists, recorded with the evidence, so the image build (E-19/c14a) and the executors (E-19/c09, E-19/c11) use one list.
- A progress sink that delivers progress lines under the input whitelist, recorded with the exact ffmpeg options.
- An HDR to SDR filter order with its measured cost per second of media on REF-CPU4, and the luma tolerance IT-10-10 will assert.

## Scope

### Included
- Run candidate whitelists (initial: `mov,matroska,avi,mpegts`; h264, hevc, vp9, av1, aac, opus, mp3) over the phone clips of DS-07, DS-08 and DS-35 and the video items of DS-16.
- Verify `-progress` output to the chosen sink with `-protocol_whitelist file` set; if it is blocked, test and record an alternative.
- Measure the chain "downscale, `zscale` linear, `tonemap=hable`, `zscale` BT.709, `yuv420p`" and at most two alternatives on DS-06 base (4K HLG) on REF-CPU4; record the luma range of the chosen output against a harness SDR render.
- Write the decisions into `components/10_decoder/description.md` §4 and §12 (risks 4 and 5 closed) and the measured numbers into the gate S5 record of E-31.

### Excluded
- Building ffmpeg with the lists (E-19/c14a) and the argv builders (E-19/c11).
- Scratch sizing for the supervisor (E-20/c01) and NVENC (gate S10, v1.1).

## Acceptance Criteria

**AC-1: Whitelists keep phone clips and refuse the red-team container tricks (AC-25)**
Given the candidate lists and the clips of DS-07, DS-08, DS-35 and the video items of DS-16
When each clip is probed and transcoded with the lists set
Then every phone clip succeeds, the valid DS-16 control video succeeds, every DS-16 video item that is not a valid MP4 (the `.m3u8` text named `.mp4`, the concat file, the SDP) is refused by the whitelists, and the final lists with the per-clip result table are recorded in the component description. The 20 GB sparse DS-16 video is a structurally valid MP4: no whitelist refuses it, and the job limits (size, duration, timeout; IT-10-20, NFT-RES-LIM-04) contain it, so it is checked there and not here (reworded 2026-11-11 by the owner)

**AC-2: Progress lines arrive under the input whitelist**
Given an ffmpeg transcode with `-protocol_whitelist file` and the chosen progress sink
When the transcode runs for at least 30 s
Then progress lines arrive at least once per second and the exact options are recorded; if the first sink is blocked, the working alternative is recorded instead

**AC-3: HDR order fits the budget (AC-11)**
Given DS-06 base on REF-CPU4 with 4 cores
When each candidate chain transcodes it to 1080p SDR
Then the wall time per second of media is recorded for each, the chosen chain is at or below 2.0 s/s, and the luma tolerance for IT-10-10 is written next to it

**AC-4: Hardware gate honoured**
Given REF-CPU4 is not reachable
When the spike is closed
Then AC-3 is recorded as NOT-RUN in `_docs/_process_leftovers/`, the chain is marked provisional, and no workstation number is recorded as the gate result

## Non-Functional Requirements

**Performance**
- 4K HLG to 1080p SDR at or below 2.0 s/s on REF-CPU4 (PT-10-2, NFT-PERF-16).

**Compatibility**
- ffmpeg 8.x/9.x with libx264, native `aac`, libzimg; no `concat`, `hls`, `sdp`, `data`, `tee`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Spike record review | NOT-RUN recorded in leftovers when REF-CPU4 is absent; no workstation number as the gate result |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | DS-07, DS-08, DS-35, DS-16 | Probe and transcode per clip with the lists | Phone clips and the DS-16 control video pass; the DS-16 container tricks are refused; the 20 GB sparse video is contained by the limits (IT-10-20) | NFT-SEC-29 step 6, NFT-RES-LIM-04 |
| AC-2 | DS-05 base | Progress lines under the whitelist | At least 1 line/s | AC-45 |
| AC-3 | DS-06 base, REF-CPU4 | Wall time per chain | Chosen chain <= 2.0 s/s | PT-10-2, NFT-PERF-16 |

## Constraints

- Speed numbers count only on REF-CPU4 (`tests/environment.md`); other hosts give functional results only.
- The decoder never receives free-form ffmpeg arguments; the decision is expressed as fixed lists and fixed filter order.

## Risks & Mitigation

**Risk 1: No chain fits 2.0 s/s**
- *Risk*: tone mapping 4K on 4 cores exceeds the budget.
- *Mitigation*: record the best result and raise it to the owner with the AC-11 number; do not lower the budget in the spike.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Gate S5 is also named for the supervisor's scratch size (09 §6); this spike does not size scratch, E-20/c01 does | E-20 | open | Low |
