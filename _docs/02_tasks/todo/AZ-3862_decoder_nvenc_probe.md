# NVENC probe by test encode and EncoderFailed reporting [v1.1]

**Task**: AZ-3862_decoder_nvenc_probe
**Name**: Decoder NVENC probe [v1.1]
**Description**: When `nvenc = on`, prove NVENC works by one real test encode at start, report it in `hello`, run `Plan` encoder `Nvenc` with `h264_nvenc` on system-memory frames within the probed session limit, and report a failed encode as `EncoderFailed{exit_code}`
**Complexity**: 2 points
**Dependencies**: AZ-3860_decoder_video_executor, AZ-3861_decoder_hello_self_check
**Component**: 10_decoder
**Tracker**: AZ-3862
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c13

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/10_decoder/decoder_protocol.md` — `Capabilities.nvenc_probe_ok`, `nvenc_sessions`, `EncoderFailed`.

## Problem

AC-28 (v1.1) allows GPU encoding only when the operator enables it and a real test encode proves it works; listing encoders proves nothing. A missing or failing GPU must never fail a job: the decoder reports a typed failure and the supervisor reruns on CPU.

## Outcome

- With `nvenc = on`, `hello` reports a probe result from a real encode; with it off, the probe never runs.
- `Nvenc` plans use `h264_nvenc` only, capped at the probed session count; failures come back as `EncoderFailed{exit_code}`.

## Scope

### Included
- At start, when `nvenc = on`: one test encode (`nullsrc` 256x256, 0.1 s, `h264_nvenc` to null), result cached; `nvenc_probe_ok` and `nvenc_sessions` in `Capabilities`.
- `Plan::Transcode{encoder: Nvenc}`: `-c:v h264_nvenc -preset p5 -tune hq -rc vbr -cq 23 -b:v 0 -profile:v high` on system-memory frames (no CUDA filters); concurrency capped at the probed session limit.
- Non-zero exit of the NVENC encode -> `EncoderFailed{exit_code}`; an `Nvenc` plan without a successful probe answers `EncoderFailed` or is rejected, never crashes.

### Excluded
- VAAPI, QSV, VideoToolbox (experimental, unused); the CPU rerun (E-20/c06); the GPU Compose override (deployment).

## Acceptance Criteria

**AC-1: No GPU, probe off by default (IT-10-20 NVENC part, AC-28)**
Given `nvenc = off`
When the runner starts and `hello` is sent
Then no test encode ran and `nvenc_probe_ok` is false

**AC-2: Enabled without a GPU (IT-10-20)**
Given `nvenc = on` on a host without a GPU
When the runner starts, `hello` is sent, and a `Plan` with `encoder: Nvenc` is sent
Then `nvenc_probe_ok` is false, the argv log shows no `h264_nvenc` use in jobs, and the plan answers `EncoderFailed` or is rejected without a crash

**AC-3: Enabled with a GPU (FT-P-28)**
Given `nvenc = on` on REF-NV
When the runner starts and a 1080p clip is transcoded with `encoder: Nvenc`
Then `nvenc_probe_ok` is true, `nvenc_sessions` equals the probed limit, the output passes the same validation as the CPU path, and a forced encode failure returns `EncoderFailed{exit_code}`

## Non-Functional Requirements

**Performance**
- At least 1.3x faster than CPU on the 1080p reference clip (AC-28, NFT-PERF-18 on REF-NV).

**Compatibility**
- No `RLIMIT_AS` (breaks NVENC below 4 GiB); works under the 4 GiB video limit (NFT-RES-LIM-09).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | NVENC argv builder (golden) | Exact flags; no CUDA filters |
| AC-2 | Plan handling with probe false | `EncoderFailed` or rejection, no panic |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | No-GPU host, `nvenc` off and on | IT-10-20 NVENC part | Probe false; no NVENC use; no crash | FT-N-08, NFT-RES-31 |
| AC-3 | REF-NV | GPU path | Probe true; valid output; typed failure | FT-P-28, NFT-PERF-18, NFT-RES-LIM-09 |

## Runtime Completeness

- Named capability: NVIDIA NVENC through ffmpeg `h264_nvenc`.
- Production code that must exist: the real test encode and the real NVENC encode path.
- Allowed external stubs: none; the GPU cases are hardware-gated (REF-NV) and recorded NOT-RUN where the hardware is absent.
- Unacceptable substitutes: `ffmpeg -encoders` listing as the probe.

## Constraints

- Release v1.1; off by default.

## Risks & Mitigation

**Risk 1: Tone-mapped HDR erases the GPU gain**
- *Risk*: CPU decode and filtering dominate, so 1.3x does not hold for HDR (10 risk 3).
- *Mitigation*: measured in gate S10; AC-28's number is for the 1080p SDR clip.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The ffmpeg build recipe (`--disable-autodetect`, no CUDA/NPP) does not name enabling the NVENC encoder or its headers; the v1.1 image needs that change to the recipe of E-19/c14a | E-19 and deployment owners | open | Low |
