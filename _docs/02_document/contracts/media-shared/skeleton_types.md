# Contract: media-shared skeleton types (job DTOs, policy types, decoder protocol types)

**Component**: `media-shared` (layer-2 type crate; `types` and `policy` owned by 08/E-18, `decoder_protocol` owned by 10/E-19)
**Producer task**: E-00/c06 — `_docs/02_tasks/todo/B01-15_media_shared_skeleton.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: other batches: E-18 (08 media jobs), E-19 (10 decoder), E-20 (09 worker supervisor)
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

The plain data types that the API side (08), the trusted supervisor (09) and the untrusted decoder (10) share, so a job, a probe or a decoder message has one definition. Types only: the policy functions (`is_compliant`, `decide`, `poster_offset_ms`), the parameter values of `Params[params_version]`, the socket framing and the range checks are behaviour of E-18 and E-19. ADR-009: no sqlx, no queue module, no I/O.

Sources: `components/08_media_jobs/description.md` §4, §5, §7; `components/09_worker_supervisor/description.md` §2; `components/10_decoder/description.md` §3; `canonical-names.md` S4-1; ADR-009, ADR-010.

## Shape

### Module `types` (job DTOs for the worker listener `:8081`)

| Type | Fields / variants | Wire use |
|---|---|---|
| `Lane` | `Image`, `Video` (serialised `image`, `video`) | claim request body `{lane}` |
| `JobKind` | `Stills`, `Poster`, `Transcode` | `ClaimedJob.kind` |
| `ClaimedJob` | `job_id: Uuid, kind: JobKind, content_hash: FileHash, params_version: u32, attempt: u32, lease_ms: u64` | 200 body of `POST /api/v1/worker-jobs/claims`; `content_hash` serialised as lowercase hex, 64 characters |
| `LeaseExtended` | `lease_ms: u64` | 200 body of `PUT .../lease` |
| `DerivedKind` | `Thumb`, `Preview`, `Transcode` | `{kind}` path segment of `PUT .../outputs/{kind}` |
| `OutputMeta` | `media_type, width?, height?, duration_ms?` | `Content-Type`, `X-Width`, `X-Height`, `X-Duration-Ms` |
| `SkipOutcome` | `SkippedCompliant`, `Obsolete` (serialised `skipped_compliant`, `obsolete`) | body of `PUT .../result` |
| `Disposition` | `Retry`, `Permanent` | body `{code, disposition}` of `POST .../failure` |
| `FailureReport` | `code: String` (short code), `disposition: Disposition` | same |
| `FailResult` | `Requeued`, `Dead` | 200 body `{result}` of `POST .../failure` |
| `WorkerStatus` | `decoder_ok: bool, image_age_days: u32` | body of `PUT .../status` |

### Module `policy` (types only)

| Type | Fields / variants |
|---|---|
| `Probe` | `kind: Photo \| Video, container, moov_first: bool, video?: VideoStream{codec, profile, width, height, rotation, bitrate, pix_fmt, primaries, transfer, matrix}, audio?: AudioStream{codec}, duration_ms?` (photos fill `width`/`height` only) |
| `Plan` | `Remux` (no parameters) or `Transcode{encoder: Cpu \| Nvenc, scale_to_short_edge, tone_map, audio, faststart}` (enum-only record, never free-form arguments) |
| `Decision` | `SkipCompliant`, `Remux`, `Transcode(Plan)` |
| `DecoderCapabilities` | the `Capabilities` reply of `hello` (below) as the policy reads it, incl. `nvenc_probe_ok` |
| `EncoderPref` | the worker-local encoder choice (`Cpu`, `Nvenc`; `Nvenc` only from v1.1) |
| `Params` | the output settings of one `params_version` (edges 320/1440, WebP quality, CRF, preset); values are E-18's |
| `OutputSpec` | what a job must produce per `DerivedKind` (media type, bounds) |

### Module `decoder_protocol` (types only; framing is E-19)

| Request | Input | Response | Errors (`DecodeError`) |
|---|---|---|---|
| `Hello` | `protocol: u32` | `Capabilities{protocol, vips, ffmpeg, libheif, build_timestamp, nvenc_probe_ok, nvenc_sessions}` | `ProtocolMismatch` |
| `SelfCheck` | `forbidden_targets: Vec<String>` (host:port) | `SelfCheckReport(Vec<{name, ok}>)` | none |
| `Probe` | `job_dir` | `Probe` | `Unsupported`, `Corrupt`, `LimitExceeded` |
| `RenderStill` | `job_dir, max_edge, poster_at_ms?, limits: Limits` | `ImageResult{width, height, bytes}` | `Unsupported`, `Corrupt`, `LimitExceeded{wall, memory, pixels, output}`, `Busy` |
| `Transcode` | `job_dir, plan: Plan, limits: Limits` | `TranscodeResult{probe_of_output: Probe, bytes}` | as `RenderStill` plus `Stalled`, `EncoderFailed{exit_code}`, `OutputInvalid` |
| `Cancel` | none | `Cancelled` | none |

`Limits` = `cpu, memory_bytes, wall, stall_timeout, max_output_bytes`. Events while a job runs: `Alive`, `Progress{out_time_ms, speed}`. `DecodeError` also has `ProtocolViolation` (any request).

## Invariants

- The crate depends on serde-level crates and `sc_ids` (for `FileHash`) only: no sqlx, no tokio I/O, no HTTP client, no `contracts`, no component crate (crate-graph test).
- Every type is serialisable to and from JSON; the `decoder_protocol` types reject unknown fields when deserialised (10 §3: the decoder is untrusted).
- `Plan` carries enumerations and bounded numbers only; no string can become an ffmpeg argument.
- The serialised names of `types` are the ones the worker routes of 08 §5 use (`image`, `video`, `skipped_compliant`, `obsolete`, `retry`, `permanent`, `requeued`, `dead`).

## Non-Goals

- Not covered: `is_compliant`, `decide`, `poster_offset_ms`, the `Params` values (E-18); frame length prefix, 64 KiB cap and numeric range checks (E-19); the HTTP client (E-20).

## Versioning Rules

- `decoder_protocol` changes bump `protocol` in `Hello` (decoder protocol v1 today). Any field change in `types` needs the same change in the API-side copy (`contracts` part 3, Open point 2) in the same task.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| json-roundtrip | one value of every type | serialise then deserialise gives an equal value | |
| unknown-field | a `decoder_protocol` reply with an extra field | rejected | 10 §3 |
| wire-names | `Lane::Image`, `SkipOutcome::SkippedCompliant`, `FailResult::Dead` | `"image"`, `"skipped_compliant"`, `"dead"` | 08 §5 |
| no-forbidden-deps | crate graph | no sqlx, contracts, component, HTTP crate | crate-graph test |

## Open points (flagged in the producer task)

1. Closed 2026-11-07 (owner decision): `ClaimedJob.content_hash` is lowercase hex, 64 characters, the encoding the HTTP API surface already uses for content hashes (api_v1_surface.md, `revisions:lookup`).
2. Integer widths (`params_version`, `attempt`, `image_age_days`) are not specified; chosen here as unsigned 32-bit.
3. `OutputSpec` and `EncoderPref` are named (10 §1, 09 §2) without fields.
4. The same job DTOs are declared in `contracts` for the API side (part 3, Open point 2).
5. The type names `LeaseExtended`, `OutputMeta`, `FailureReport` and `Decision` are introduced here for bodies and results that 08 §5 and 09 §2 describe without a type name.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
| 1.0.1 | 2026-11-07 | Closed open point 1: `ClaimedJob.content_hash` is lowercase hex, 64 characters | AZ-3826 |
