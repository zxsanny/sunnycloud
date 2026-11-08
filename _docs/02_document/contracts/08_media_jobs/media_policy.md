# Contract: media-shared policy (compliance, decision, poster offset, supported kinds)

**Component**: 08_media_jobs (`media-shared::policy`)
**Producer task**: E-18/c03 — AZ-3838 (`_docs/02_tasks/todo/AZ-3838_media_policy.md`)
**Consumer tasks**: E-20/c06 (poster offset in the slot loop), E-20/c10 (policy wiring and output compliance check); in batch E-18/c06, E-18/c12
**Version**: 1.1.0
**Status**: implemented in `src/media-shared/src/policy/rules.rs`
**Last Updated**: 2026-11-07

## Purpose

The pure rules that decide what happens to a clip (skip, remux or transcode), where a poster frame is taken, and which jobs a media type gets. The supervisor applies them to the decoder's probe; the API planner uses `supported_kinds_for`. The decoder never calls them.

## Shape

| Name | Signature | Errors | Blocking? |
|------|-----------|--------|-----------|
| `is_compliant` | `(&Probe) -> bool` | none | sync, pure |
| `decide` | `(&Probe, &DecoderCapabilities, EncoderPref) -> Decision{SkipCompliant \| Remux \| Transcode(Plan)}` | none | sync, pure |
| `cpu_transcode_plan` | `(&Probe) -> Plan` | none | sync, pure |
| `poster_offset_ms` | `(duration_ms: u64) -> u64` | none | sync, pure |
| `supported_kinds_for` | `(media_type: &str) -> Vec<JobKind>` | none | sync, pure |

### Compliance axes (all must hold)

| Axis | Compliant when |
|------|----------------|
| Video codec and profile | H.264, High profile or lower (Baseline, Main, High) |
| Video bitrate | at most 10,000,000 bit/s (decimal, inclusive) |
| Size | long side at most 1920 and short side at most 1080, whatever the rotation metadata |
| Audio | AAC, or no audio |
| Colour | SDR BT.709 |
| Container | MP4 with the index (`moov`) first |

How the axes read the `Probe` (all comparisons ASCII case-insensitive):

- A `Probe` that is not `kind: video`, has no video stream, or leaves `codec`, `profile`, `bitrate`, `primaries`, `transfer` or `matrix` unreported is not compliant (the rule cannot prove the axis).
- Codec `h264`; profile exactly `Baseline`, `Main` or `High` (so `High 10`, `Constrained Baseline`, `Extended` are not compliant); AAC audio codec `aac`.
- Colour: `primaries`, `transfer` and `matrix` are all `bt709`. `pix_fmt` is not an axis.
- Size: the two sides are ordered before the comparison, so `rotation` is never read.
- Container: the comma-separated `container` names (ffprobe `format_name`, e.g. `mov,mp4,m4a,3gp,3g2,mj2`) must include `mp4`.
- HDR (for `tone_map`): `transfer` is `smpte2084` (PQ) or `arib-std-b67` (HLG).

### Decision

| Probe | Decision |
|-------|----------|
| every axis holds | `SkipCompliant` (never re-encoded) |
| every axis holds except the index is at the end | `Remux` (lossless copy of the first video and audio stream, index moved to the front) |
| otherwise | `Transcode(Plan)`: H.264 High, size bound, AAC, faststart, HDR tone-mapped to SDR, encoder from `EncoderPref` |

`Plan::Transcode` fields: `encoder` = `EncoderPref`; `scale_to_short_edge` = 1080 (`Params` v1 `max_short_side`); `tone_map` = HDR probe; `audio` = `Aac` when the probe has an audio stream, `None` when it has none; `faststart` = true. The `DecoderCapabilities` argument is not read by any v1 rule.

### Poster offset

1,000 ms, or 10% of the duration for clips shorter than 10 s; never beyond the duration.

### Supported kinds

| Media types (by format; the exact type strings are those of 03's commit-time sniffer) | Kinds |
|-------------|-------|
| JPEG, PNG, WebP, GIF, HEIC | `[stills]` |
| MP4, MOV, MKV | `[poster, transcode]` |

Exact strings matched (lowercase, no parameters, like `OutputSpec::accepts`): `image/jpeg`, `image/png`, `image/webp`, `image/gif`, `image/heic` give `[stills]`; `video/mp4`, `video/quicktime`, `video/x-matroska` give `[poster, transcode]`. Anything else, including `image/svg+xml`, `image/avif`, `image/heif` and `video/webm`, gives `[]`.
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

1. Kinds for AVIF, WebM and HEIF variants are not stated (they get `[]`).
2. `decide` takes decoder capabilities; no v1.0 rule reads them (the NVENC choice is v1.1).
3. Assumptions to confirm against the real decoder probe (IT-08-17, E-18/c12) and 03's sniffer: the exact media type strings above; the `container` convention (a MOV file reports the same `format_name` as MP4, so the policy cannot tell them apart); `Constrained Baseline` and `Extended` profile names; an unreported bitrate or colour tag counts as not compliant; the HDR transfer names; `audio: none` in the plan of a clip without audio.

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B05) |
| 1.0.1 | 2026-11-07 | Implemented; probe-reading rules, plan fields and exact media type strings written down; open point 3 | AZ-3838 |
| 1.1.0 | 2026-11-08 | `cpu_transcode_plan`: the plan a failed `Remux` or failed hardware encode is rerun with (same fields as the `Transcode` of `decide`, encoder fixed to the CPU) | AZ-3872 |
