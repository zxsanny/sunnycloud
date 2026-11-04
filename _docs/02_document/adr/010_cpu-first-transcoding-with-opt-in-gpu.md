# ADR-010: CPU-first transcoding with opt-in GPU

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

Operators install on their own hardware and want videos from phones to play in every browser and app. A phone clip is often HEVC, 4K, HDR, or an MP4 whose index sits at the end of the file. Transcoding is the most expensive job the server runs, and the acceptance budget is written for a CPU-only worker with 4 physical cores. The research measured CPU 1080p SDR at 3.3x real time, 4K HLG to 1080p SDR at 0.63x real time, and NVENC at only 1.4x on 1080p because the filters stay on the CPU (`01_solution/solution.md` § Assessment Findings, rows "AC-11 reference clip" and "NVENC default-on").

- Acceptance criteria addressed: AC-11 (10 min 1080p SDR in 5 min, 10 min 4K HLG in 20 min, compliant clips never re-encoded, trailing index remuxed), AC-44 (derived storage at most 20% of original bytes), AC-45 (stalled ffmpeg killed within 90 s, orientation and A/V drift at most 40 ms), AC-28 (NVENC at least 1.3x, test-encode probe, CPU fallback, v1.1), AC-10 (poster frame is a thumbnail)
- Restrictions addressed: RESTRICT-13 (H.264 High 1080p MP4, transcode only when not compliant, no `libfdk_aac`, no nonfree), RESTRICT-14 (NVENC opt-in, CPU is the acceptance baseline), RESTRICT-28 (operator hardware, streaming out of scope), RESTRICT-04 (Intel/AMD/Apple GPU experimental only), RESTRICT-02 (NVIDIA in v1.1)
- Risks addressed: R03 (AC-11 unproven without reference hardware), R20 (GPL source offer and codec patents for the shipped image)
- Research source: `_docs/01_solution/solution.md` § Component: Media pipeline (Video rows); `_docs/00_research/raw/media_pipeline.md` § T2

## Decision

We will transcode on the operator's CPU to one derivative format, H.264 High 8-bit, 1080p-bounded, AAC, MP4 with the index first, using libx264 and ffmpeg's native `aac` encoder, and only when the original is not already compliant; NVENC is an opt-in v1.1 add-on.

The supervisor applies the policy (`media-shared::policy`, constants versioned by `params_version`); the decoder only probes and executes the plan. Compliant means H.264 High or lower, video at most 10 Mbit/s, AAC or no audio, SDR BT.709, index first, and an orientation-independent bound of long side at most 1920 and short side at most 1080 (rotation metadata does not matter). The decision is `SkipCompliant` (no re-encode, no output), `Remux` when everything holds except the index is at the end (stream copy with `-movflags +faststart`), otherwise `Transcode`: `libx264 -preset veryfast -crf 23 -profile:v high -maxrate 10M`, HDR tone-mapped to SDR after downscaling, and `-fps_mode vfr` with `aresample=async=1` for drift. The poster frame is a `thumb` rendered at 1 s, or at 10% of the duration for clips shorter than 10 s.

ffmpeg is built `--enable-gpl --enable-libx264 --enable-libzimg` without `--enable-nonfree`, `libfdk_aac` or `libx265`. NVENC (`h264_nvenc`, system-memory frames, no CUDA filters) is off unless `SUNNYCLOUD_NVENC=on`, is detected by a real test encode, and falls back to CPU on any failure. There is no streaming in v1.0: the product serves one MP4 per video.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| HEVC output | 2-3x the x264 encode cost (inference) and patent-pool exposure for software encoders; browser support only "partial" (`raw/media_pipeline.md` § T2); puts the AC-11 5 min budget on 4 cores at risk |
| AV1 output (SVT-AV1) | Hardware decode on about 8.5% of devices in 2024 and iOS needs A17 Pro or newer (`raw/media_pipeline.md` § T2); kept only as a possible later archive tier |
| HLS or other adaptive streaming | Out of scope (RESTRICT-28, AC "Out of scope: Video streaming"); Immich, a funded team, still calls HLS incomplete in 2026 (`raw/b_ops_licence_scope.md` finding F2) |
| Always transcode | Violates AC-11 (a compliant clip is never re-encoded), loses quality and spends worker time; the 0.63x rate for 4K HLG would be paid on every clip |
| GPU-first (NVENC default) | Gain is 1.4x on 1080p with CPU filters, container fragility, `RLIMIT_AS` below 4 GiB breaks it; CPU already runs 3.3x real time for 1080p SDR |
| Cloud transcoding | Contradicts operator hardware (RESTRICT-28) and the no-cloud-only-services rule in `architecture.md` § 2 |

## Consequences

### Positive

- One output format plays everywhere, one policy function, one `params_version`; phones' compliant clips cost zero encode time and the remux path is lossless.
- The 20-minute budget for 10 minutes of 4K HLG is met on paper by about 16 minutes at 0.63x real time, 21% headroom.
- The ffmpeg build is redistributable under GPL with no nonfree parts (R20 mitigated by a per-build source tarball, see ADR-011).
- Adding workers raises throughput with no code change (NFT-PERF-17, at least 1.7x for 2 workers).

### Negative

- CPU transcoding of 4K HDR is slow and uses all four cores for up to 20 minutes per 10-minute clip, so a bulk video import queues for hours.
- Output is H.264 only: files are larger than HEVC or AV1 would give, and AC-44's 20% cap depends on CRF and preset tuning not yet done (NFT-PERF-20).
- Users who want streaming or multiple renditions get neither in v1.0.
- NVENC adds a second code path, a test-encode probe, a Compose override (`compose.gpu.yml`) and a CPU fallback to test (gates S5, S10).

### Neutral / Open

- The poster-frame SSIM tolerance against the reference render is an open owner item (`canonical-names.md` § Open owner items).
- Intel, AMD and Apple encoders stay experimental and unused; no test hardware (D6).
- The decoder holds no policy: changing any constant bumps `params_version` and creates new derived rows.

## Evidence

- `_docs/02_document/component-decisions.md` § Decisions (items 2, 3 and 14)
- `_docs/02_document/components/08_media_jobs/description.md` § 7 Transcode Policy and Output Specs
- `_docs/02_document/components/09_worker_supervisor/description.md` § 3 Behaviour (job flow)
- `_docs/02_document/components/10_decoder/description.md` § 4 Execution profiles (Video poster; Video)
- `_docs/02_document/deployment/containerization.md` § Images (Decoder: ffmpeg build; GPU)
- `_docs/02_document/system-flows.md` § Flow F10: Media pipeline
- `_docs/00_research/06_component_fit_matrix.md` § Top-level matrix (Video rows); § Round 2 changes (NVENC)
- `_docs/00_research/04_reasoning_chain.md` § Dimension 6: Media pipeline
- `_docs/02_document/risk_mitigations.md` § Risk Register (R03, R20)

## Notes

Re-evaluate the NVENC default only if a measured run with GPU scaling (not CPU filters) shows a clearly larger gain than 1.4x. The AC-28 margin of 1.3x against a measured 1.4x is thin.
