# media-shared policy: compliance, decision, poster offset, supported kinds

**Task**: AZ-3838_media_policy
**Name**: Transcode and media policy
**Description**: Pure policy functions that decide whether a clip is already compliant, whether it needs a lossless remux or a transcode, where the poster frame is taken, and which job kinds a media type gets
**Complexity**: 3 points
**Dependencies**: AZ-3837_media_shared_params_protocol
**Component**: 08 media_jobs (`media-shared::policy`)
**Tracker**: AZ-3838
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c03

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md` — `Params[1]` and output specs (E-18/c02).
- `_docs/02_document/contracts/media-shared/skeleton_types.md` — `Probe`, `Plan`, `Decision`, `EncoderPref` (E-00/c06).

## Problem

A compliant clip must never be re-encoded, and a compliant clip with its index at the end must be remuxed losslessly (AC-11). The supervisor applies the rule to the decoder's probe, and the API's planner needs the same view of which media types get which jobs. The rule must be one pure function set used by both, with the boundary cases fixed by the gap pass (decimal 10 Mbit/s, inclusive).

## Outcome

- `is_compliant(&Probe)`: H.264 High profile or lower, video bitrate at most 10,000,000 bit/s (inclusive), AAC or no audio, SDR BT.709, MP4 with the index first, long side at most 1920 and short side at most 1080 whatever the rotation metadata.
- `decide(probe, caps, encoder_pref)`: `SkipCompliant` when every axis holds; `Remux` when only the index is at the end; otherwise `Transcode(Plan)` with H.264 High, the size bound, AAC, faststart and tone mapping for HDR.
- `poster_offset_ms(duration_ms)`: 1,000 ms, or 10% of the duration for clips shorter than 10 s, never beyond the duration.
- `supported_kinds_for(media_type)`: `[stills]` for photos, `[poster, transcode]` for videos, `[]` otherwise.

## Scope

### Included
- The four functions and their table tests with synthetic probes.
- The contract file `contracts/08_media_jobs/media_policy.md`.

### Excluded
- Running them on real decoder probes (IT-08-17 real-probe part in E-18/c12).
- Applying the policy in the supervisor (E-20/c10); the planner (E-18/c06).

## Acceptance Criteria

**AC-1: Compliance boundaries (AC-11)**
Given synthetic probes at exactly 10,000,000 bit/s, 1920x1080, 1080x1920, profiles Baseline, Main and High, and no audio
When `is_compliant` runs
Then each is compliant; and 10.5 Mbit/s, 1920x1088, High 10, HEVC, MP3 audio and HLG BT.2020 are each not compliant; rotation metadata never changes the result

**AC-2: Decision**
Given a compliant probe, a probe compliant except for the index at the end, and the non-compliant probes of AC-1
When `decide` runs with the CPU encoder
Then it returns `SkipCompliant`, `Remux` (no parameters) and `Transcode` plans with H.264 High, short side at most 1080, long side at most 1920, AAC and faststart, with tone mapping only for the HDR probe

**AC-3: Encoder preference changes only the encoder (AC-28)**
Given the same non-compliant probe
When `decide` runs with `Cpu` and with `Nvenc`
Then the two plans differ only in the encoder

**AC-4: Poster offset**
Given durations 0, 500, 5,000, 9,999, 10,000, 10,001 and 600,000 ms
When `poster_offset_ms` runs
Then it returns 0, 50, 500, 999, 1,000, 1,000 and 1,000, never above the duration

**AC-5: Supported kinds**
Given JPEG, PNG, WebP, GIF, HEIC, MP4, MOV, MKV, SVG and `application/octet-stream`
When `supported_kinds_for` runs
Then the photo types give `[stills]`, the video types give `[poster, transcode]`, and SVG and octet-stream give `[]`

## Non-Functional Requirements

**Reliability**
- Pure functions: no I/O, no clock, no randomness.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Compliance table per axis and boundary | As listed |
| AC-2 | Decision table | Skip, remux, transcode as listed |
| AC-3 | Plan diff by encoder | Encoder only |
| AC-4 | Poster offset table | As listed |
| AC-5 | Kinds per media type | As listed |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Synthetic boundary probes | IT-08-17 synthetic part | As AC-1, AC-2 | FT-P-25, NFT-PERF-17 |
| AC-4 | Durations | IT-08-18 | As AC-4 | |
| AC-5 | Media type list | IT-08-1 `supported_kinds_for` clause | As AC-5 | |

## Runtime Completeness

- Named capability: the transcode compliance and remux rule (AC-11).
- Production code that must exist: the real decision functions, used unchanged by the supervisor.
- Unacceptable substitutes: a decision that always transcodes, or a table that skips the remux case.

## Constraints

- The policy is applied by 09, never by the decoder (10 does not call `policy`).
- Changing any number bumps `params_version`.

## Risks & Mitigation

**Risk 1: Probe fields differ from real ffprobe output**
- *Risk*: synthetic probes pass while real ones fail.
- *Mitigation*: IT-08-17 reruns the table on real probes of DS-07 and DS-35 in E-18/c12.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Media types beyond the IT-08-1 list (AVIF, WebM, HEIF variants) have no stated kind | E-18 owner | open | Low |
| `decide` takes decoder capabilities, but 08 §7 names no rule that reads them other than the v1.1 NVENC choice | E-18 / E-20 owners | open | Low |

## Contract

This task produces `_docs/02_document/contracts/08_media_jobs/media_policy.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
