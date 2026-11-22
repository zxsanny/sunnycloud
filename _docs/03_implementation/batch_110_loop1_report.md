# Batch 110 (loop 1): host-process decoder gaps of AZ-3854 to AZ-3859 (roadmap item 4, slice 3)

Owner decision 2026-11-22: host decoder gaps next. The HLG SSIM point of AZ-3859 AC-4 records only; no tolerance needed.

## Stall kill (AZ-3854 AC-6), the one product change

`Spec.progress` makes the guard read the child's stdout file (`-progress pipe:1`, gate S5) as it grows. Each completed block
(`progress=continue|end`) resets the stall timer and is forwarded as a `Progress` event (at most one per 100 ms). With no block for
`stall_timeout` the whole group is killed as `Kill::Stall`, which maps to `DecodeError::Stalled` (hostile class). A child that
keeps writing blocks is never killed. Reads are bounded per poll (64 KiB) and lines are capped (256 bytes). Files:
`src/decoder/src/process.rs`, `progress.rs` (new), `failure.rs`, `exec.rs`, `lib.rs`. `Finished` carries `Timing {started,
last_progress, decided}` from the guard's own clock, so a kill is timed against the instant the guard acted, not against when a
test noticed. `Spec.progress` is false for every current caller (there is no transcode): AZ-3860 must set it and pass `-progress pipe:1`.
AZ-3849 AC-2 in miniature: real ffmpeg 8.0.1 under `-protocol_whitelist file` gave 9 blocks in 5.7 s, no gap of 3 s; the same ffmpeg
stopped with SIGSTOP mid-run is `Killed(Stall)` 3 to under 4 s after its last block, its group gone.

## Per task (tests added; the host-process tests live in `src/decoder/tests/` and `src/decoder/src/process/tests/`)

| Task | Done now | Stays open |
|---|---|---|
| AZ-3854 | AC-4 wall (guard-recorded kill in 3 to 4 s); AC-5 512 MiB against a 128 MiB limit killed within 1 s of the child's own stamp; AC-6 stall (print then stop, steady child for 10 s, no block, tool without a sink, two real-ffmpeg tests); AC-7 1 GiB `dd` stopped at 1 MiB by SIGXFSZ; AC-9 1 MiB of ANSI stderr leaves a tail of at most 2048 bytes | the idle-CPU check (PT-10-3, perf tier) and the memory poll under CPU starvation are not asserted |
| AZ-3855 | AC-2 and the 1 s bound of AC-1: `an_escaped_descendant_is_dead_within_a_second_of_the_tool_ending_and_cannot_read_the_next_job` (no live descendant at the reply, `work/` gone, reply under 1 s after the tool ended, `leak.txt` absent after job B on the restarted runner); AC-3, AC-4 structure existed | the runner exit within 5 s of AC-4 is only observed within 20 s here (asserted at 5 s in CI by the container suite) |
| AZ-3857 | orientation 1 to 8 at 320 and 1440 (upright, SSIM at least 0.95, lowest 0.9965 against an upright reference); EXIF, GPS, maker string and ICC are stripped (proved with `vipsheader -a` and a RIFF chunk parser, after asserting the source has them); 4 HEIC photos (lowest SSIM 0.9837); 1000x1000 renders at 41 MiB, 1001x1000 refused under 1 s, the 900-megapixel header-only bomb refused under 2 s at 36 MiB | WebP quality has no source (`render_still` carries no `params_version`; the runner uses a fixed `WEBP_QUALITY = 80`): owner decision, protocol open point 7 |
| AZ-3858 | playlist and concat files are typed failures; the recording ffprobe wrapper shows the exact whitelist argv with the fixed input; no `/etc/passwd` bytes in any reply or runner log; 5 clips match an independent ffprobe on every named field, `moov_first` included; photo probes match `vipsheader`, runner growth 0 MiB, truncated JPEG answers Corrupt or its intact dimensions | the 50 ms and 100 ms product bounds are only recorded (250 to 310 ms measured on this loaded host); asserted instead: a 12 MP or 900 MP probe is no slower than a 640x480 probe by more than 150 ms (best of 5) |
| AZ-3859 | AC-2 output bytes equal with and without an offset; AC-3 recorded vips calls only read `work/poster.png` or `out/result.webp`, never `in/source` (an image job is the control that shows it); AC-4 HLG poster is sRGB, SSIM 1.0000 against the harness chain and 0.9062 against no tone map (recorded only) | none |

Helpers moved to `tests/common/reference.rs` and `tests/common/generated.rs` (re-exported by `container/refs.rs`); `MediaRunner::restart_with_path` added.

## Verification (re-run by the parent)

| Check | Result |
|---|---|
| fmt, clippy `--workspace --all-targets -D warnings`, clippy `decoder-runner --features test-freeze` | clean |
| `cargo nextest run -p decoder-runner -p sc_error` | 187 passed, 41 ignored (the container cases) |
| worker: the container suite after rebuilding the release image | 39 of 40 on a host at load average 41; the failing case `it_10_16_a_peer_protocol_violation_ends_the_runner` missed its 5 s exit bound and passed alone twice in 0.8 and 0.9 s (wall-clock bound under load, see batch 109 finding 8) |
| worker: `check-decoder-image.sh` | OK |
| CI (Ubuntu 24.04: libvips 8.15, ffmpeg 6.1) | pending; worker measured libvips 8.15.1 and ffmpeg 6.1.1 in an `ubuntu:24.04` container for the orientation reference (lowest SSIM 0.9963), but not the runner binary (glibc), not `-progress pipe:1` on 6.1, not the HLG clip and zscale |

## Code Review Verdict

Not run: pending the batch review of the loop.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The poster's one-frame ffmpeg does not use the stall rule (assumed transcode only, as the spec says) | AZ-3854 AC-6 | owner confirms | unclear |
| 2 | WebP quality has no source: fixed 80 | AZ-3857, protocol open point 7 | a `params_version` on the plan, shared with AZ-3860 | unclear (owner) |
| 3 | G-10-6 HLG poster SSIM tolerance: the recorded values (1.0000 against the harness chain, 0.9062 without tone map) are the input | AZ-3859 AC-4 | owner sets a tolerance when ready; no code waits for it | unclear |
| 4 | IT-10-13 (stall kill in the container suite) stays NOT RUN: no transcode starts ffmpeg with `-progress` | AZ-3865 | after AZ-3860 | clear |
| 5 | Probe latency bounds (50 ms, 100 ms) are not asserted on a loaded host | AZ-3858 | assert on a quiet host (CI runs the host-process tests) or in the perf tier | clear |

## Commit

`[AZ-3854] [AZ-3855] [AZ-3857] [AZ-3858] [AZ-3859] Decoder: stall kill with the ffmpeg progress sink, and the host-process test gaps` — Loop: 1
