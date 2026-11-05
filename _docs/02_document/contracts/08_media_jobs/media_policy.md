# Contract: media-shared policy (compliance, decision, poster offset, supported kinds)

**Component**: 08_media_jobs (`media-shared::policy`)
**Producer task**: E-18/c03 — `_docs/02_tasks/todo/B05-14_media_policy.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-20/c06 (poster offset in the slot loop), E-20/c10 (policy wiring and output compliance check); in batch E-18/c06, E-18/c12
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

The pure rules that decide what happens to a clip (skip, remux or transcode), where a poster frame is taken, and which jobs a media type gets. The supervisor applies them to the decoder's probe; the API planner uses `supported_kinds_for`. The decoder never calls them.

## Shape

| Name | Signature | Errors | Blocking? |
|------|-----------|--------|-----------|
| `is_compliant` | `(&Probe) -> bool` | none | sync, pure |
| `decide` | `(&Probe, &DecoderCapabilities, EncoderPref) -> Decision{SkipCompliant \| Remux \| Transcode(Plan)}` | none | sync, pure |
| `poster_offset_ms` | `(duration_ms: u64) -> u64` | none | sync, pure |
| `supported_kinds_for` | `(media_type) -> Vec<JobKind>` | none | sync, pure |

### Compliance axes (all must hold)

| Axis | Compliant when |
|------|----------------|
| Video codec and profile | H.264, High profile or lower (Baseline, Main, High) |
| Video bitrate | at most 10,000,000 bit/s (decimal, inclusive) |
| Size | long side at most 1920 and short side at most 1080, whatever the rotation metadata |
| Audio | AAC, or no audio |
| Colour | SDR BT.709 |
| Container | MP4 with the index (`moov`) first |

### Decision

| Probe | Decision |
|-------|----------|
| every axis holds | `SkipCompliant` (never re-encoded) |
| every axis holds except the index is at the end | `Remux` (lossless copy of the first video and audio stream, index moved to the front) |
| otherwise | `Transcode(Plan)`: H.264 High, size bound, AAC, faststart, HDR tone-mapped to SDR, encoder from `EncoderPref` |

### Poster offset

1,000 ms, or 10% of the duration for clips shorter than 10 s; never beyond the duration.

### Supported kinds

| Media types (by format; the exact type strings are those of 03's commit-time sniffer) | Kinds |
|-------------|-------|
| JPEG, PNG, WebP, GIF, HEIC | `[stills]` |
| MP4, MOV, MKV | `[poster, transcode]` |
| SVG, `application/octet-stream`, every other type | `[]` |

## Invariants

- Pure: no I/O, no clock, no randomness; same input, same output.
- `EncoderPref` changes only the encoder of a `Transcode` plan, never any other field (AC-28).
- The same `is_compliant` is applied by the supervisor to the probe of every remux or transcode output.
- A `Plan` carries enumerations, booleans and bounded numbers only.

## Non-Goals

- Not covered: running ffmpeg or libvips (10), the `Params` values (`worker_job_protocol.md`).

## Versioning Rules

- **Breaking changes** (an axis or boundary changed, a type moved between kinds) require a new major version, a `params_version` bump where outputs change, and notice to E-20.
- **Non-breaking additions** (a new supported type) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| boundary-compliant | 10,000,000 bit/s; 1920x1080; 1080x1920; no audio | compliant | IT-08-17 |
| near-miss | 10.5 Mbit/s; 1920x1088; High 10; HEVC; MP3; HLG BT.2020 | `Transcode` | IT-08-17 |
| index-at-end | compliant except `moov` at the end | `Remux` | AC-11 |
| encoder-only | same probe, `Cpu` and `Nvenc` | plans differ only in encoder | AC-28 |
| poster | 0, 500, 9,999, 10,000, 600,000 ms | 0, 50, 999, 1,000, 1,000 | IT-08-18 |
| kinds | JPEG, MP4, SVG | `[stills]`, `[poster, transcode]`, `[]` | IT-08-1 |

## Open points

1. Kinds for AVIF, WebM and HEIF variants are not stated.
2. `decide` takes decoder capabilities; no v1.0 rule reads them (the NVENC choice is v1.1).

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
