# Batch 111 (loop 1): the H.264 transcode, AZ-3860 (roadmap item 4, slice 4)

Owner decisions of 2026-11-22: a parameter-set version on the plan; a clean encoder failure is `EncoderFailed` with one CPU retry;
bitrate settings that stay under the compliance bound; hello protocol number 2; non-BT.709 SDR is converted.

## Protocol change (media-shared, contract decoder_protocol 1.1.0)

- `render_still` and `Plan::Transcode` carry `params_version: u32` (range 1..=u32::MAX; 0 is an OutOfRange violation). `Plan::Remux`
  carries none (it reads no quality value). An unknown version is answered `DecodeError::ProtocolMismatch`: clean, no child started,
  the runner keeps serving, not hostile-class. `hello.protocol` is 2 (decoder and worker). Open point 7 is resolved.
- The decoder holds the `Params` table (`src/decoder/src/params.rs`, never from env). Version 1: WebP quality 80 (the old fixed value; the
  worker contract gives no number), `libx264 -preset veryfast -crf 23 -profile:v high -pix_fmt yuv420p -maxrate 8M -bufsize 2M`,
  `-c:a aac -b:a 128k -ac 2`, `-movflags +faststart`, BT.709 tags, long edge 1920 (a unit test ties it to the media-shared policy cap).
  CRF and preset are untuned (worker_job_protocol open point 4).
- All producers, consumers, tests and corpus JSON updated in media-shared, decoder and worker; docs: decoder_protocol, worker_job_protocol 1.0.3,
  skeleton_types, media_policy, the 09 and 10 descriptions, decoder `tests.md`, the E-19 epic and the AZ-3860 spec.

## Bitrate (owner decision: lower cap, small buffer)

The compliance rule needs an output of at most 10 Mbit/s inclusive. With the spec's `-maxrate 10M -bufsize 20M` a 4 s noisy 1080p clip
averaged 13.9 Mbit/s (worst 1 s window 17.4) and would have been rejected as permanently invalid. With `-maxrate 8M -bufsize 2M`:
4 s noisy 1080p 8.38 Mbit/s, 20 s noisy 720p 7.99, the container IT-10-8 phone clip 8.77 (output to input ratio 0.48 instead of 0.69).
New tests assert at most 10 Mbit/s as ffprobe reports it and `is_compliant` on a 4 s and a 20 s saturating clip.

## Transcode design

One ffmpeg child through the guard with `Spec.progress` and `-progress pipe:1 -nostats`, so the stall, wall and cancel kills apply. The output
size comes from the input probe in Rust (short side capped by the plan, long side by the table, aspect kept, even, no upscale, rotation swaps sides).
HDR is tone-mapped to SDR with the existing zscale/tonemap chain. SDR input tagged other than BT.709 is converted (zscale primaries, transfer, matrix,
range); BT.709 or untagged input gets the retag only; probe text from an untrusted file only reaches an argument through listed tokens. The output is
opened no-follow and checked: H.264 in MP4, duration within 1 s of the input, else `OutputInvalid`. A clean non-zero ffmpeg exit is
`EncoderFailed{exit_code}` (also for a remux, which the supervisor reruns as a CPU transcode); not hostile-class. The supervisor rule already existed
(`src/worker/src/slot_loop/failure.rs`: one CPU rerun, a second `EncoderFailed` is permanent): tested, and the 09 line corrected. `Encoder::Nvenc` and
non-video input answer `Unsupported` (AZ-3862 not built). Files: `params.rs`, `video.rs`, `video_args/{mod,colour,size}.rs`, plus changes in `handler.rs`, `job.rs`,
`failure.rs`, `ffprobe.rs`, `poster.rs`, `image.rs`, `connection.rs`, `lib.rs`.

## Tests

Host-process: `video_transcode.rs` (landscape, portrait, 1080p, odd sides, no audio, metadata clip, remux, HLG, rotations by SSIM against the harness render,
VFR beeps, BT.601 conversion at PSNR 40.9 dB and SSIM 0.984 against the harness render, 18.2 dB against a retag-only render, the BT.709 clip unchanged),
`video_kills.rs` (stall by SIGSTOP killed 3001 ms after the last block, wall, cancel, clean encoder failure with the runner still serving, unknown version, Nvenc
and non-video refusal, shell stubs for four bad outputs). Container suite `it_transcode.rs`: IT-10-8, IT-10-9, IT-10-10 (synthetic 4K HLG), IT-10-11, IT-10-13a
(stall kill), IT-10-19 real-transcode form (cancel answered in 12 ms).

## Verification (re-run by the parent)

| Check | Result |
|---|---|
| fmt, clippy (workspace, and `decoder-runner --features test-freeze`), `cargo deny check`, `check-pinning.sh` | clean |
| `cargo nextest run -p decoder-runner -p sc_error -p media-shared -p sunnycloud-worker` | 583 passed, 47 skipped (the container cases) |
| worker: the container suite after rebuilding the release image | 45 of 46: `it_10_18_work_of_the_interrupted_job...` missed a 5.9 s abort bound at a host load average of 70 and passed alone in 4.6 s; the earlier run's one failure (`it_10_14...`) passed alone in 2.1 s |
| worker: `check-decoder-image.sh`, `run-script-tests.sh` | OK |
| the remux test 40 times alone (parent) | 40 of 40 |
| CI (Ubuntu 24.04, ffmpeg 6.1, libvips 8.15) | pending: zscale `pin`/`tin`/`min`, `setparams`, `-fps_mode`, `-display_rotation`, `silencedetect`, the select syntax, `-progress pipe:1` under the file whitelist, the HLG and BT.601 clip generation |

## Code Review Verdict

Not run: pending the batch review of the loop.

## NOT RUN

IT-10-13b (the 90 s default stall, about 100 s wall, outside the `--run-ignored only` gate); real DS-05, DS-06, DS-08, DS-35 files and a 30 s 4K HLG clip;
PT-10-2 speed budgets (REF-CPU4); AC-6 in the test-stubs image (covered on the host with shell stubs); the `-noautorotate` argv check in the stub image.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | One intermittent `OutputInvalid` in the remux test in a parallel run on a loaded host; not reproduced (40 of 40 alone) | AZ-3860 | leftover 2026-11-22_remux-output-invalid-once.md: read the printed runner log on a second occurrence | unclear |
| 2 | The 20 % wall-time bound of the remux against a transcode (AC-3) is only recorded (remux 90 ms against 212 ms here) | AZ-3860 AC-3 | assert on a quiet host (CI) | clear |
| 3 | A colour tag outside the conversion lists (PQ or HLG on a non-tone-mapped plan) is only retagged, by design | AZ-3860 | none | clear |
| 4 | CRF and preset are untuned: derived storage at most 20 % of originals (NFT-PERF-20) is not measured on real clips | worker_job_protocol open point 4 | tune with real phone clips and bump `params_version` | unclear |
| 5 | AC-1 also says the output stays at or below 10 Mbit/s in the worst 1 s window; a buffer lets a window carry up to maxrate plus bufsize, so `-maxrate 8M -bufsize 4M` bounded it at 12 Mbit/s | AZ-3860 AC-1 | RESOLVED 2026-11-22 (owner): bufsize 2M (8M + 2M = 10M). Measured over 5 runs each: 4 s noisy 1080p average 7.95 to 8.06, worst window 9.31 to 9.40 Mbit; 20 s noisy 720p 7.63 to 7.78 and 9.26 to 9.61; IT-10-8 phone clip 7.5 to 8.0 and 8.0 to 9.1; output to input size ratio 0.40 to 0.45 (0.48 with a 4M buffer). The tests assert the worst window at most 10 Mbit, the average at most 10, the ffprobe bit_rate and `is_compliant` | clear |

## Commit

`[AZ-3860] Decoder: H.264 transcode with a parameter-set version, EncoderFailed with one CPU retry, bitrate under the compliance bound, protocol 2` — Loop: 1
