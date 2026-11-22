# Batch 109 (loop 1): the container suite H3 (AZ-3865, slice 2 of roadmap item 4)

Owner decision 2026-11-22: the container suite next. It runs the decoder's cases inside the real image with production flags.

## What was built

- **Harness** `src/decoder/tests/container/` (Rust, `#[ignore]` unless `DECODER_IMAGE` is set, so a plain `cargo test` never passes silently): starts the release image (or the test-stubs variant) with the production flags of `compose.dev.yml` through the `docker` CLI, a protocol client on `media-shared::decoder_protocol`, per-test job directories, reference outputs from the harness's own host tools (never from the runner's report). AC-1: the started containers are inspected first and a seeded container without the production flags is refused.
- **Test-stubs variant**: Dockerfile target `test-stubs` (base is an ARG, so CI layers it on the release image it already built: one Rust build of the runner with the `test-freeze` feature plus one thin layer, no second media build), `src/decoder/stub/` (`decoder-test-stub`: hang, alloc, ignore-term, setsid fork, write to the next job, ptrace, 1 MiB stderr, write, spin, segv, exit; marker parser shared with the harness), real tools kept as `*.real`. The release image has no stub and no `*.real` (the single proof is the `no-test-hooks` property of `check-decoder-image.sh`).
- **CI**: job `decoder-suite` after `images` (same pinned actions, 40 min timeout, watchdog wrapper) running `scripts/ci/decoder-suite.sh`; a clippy step for `decoder-runner --features test-freeze` in `rust-lint.sh`; `check_pinning.py` now resolves `FROM ${ARG}` to the ARG's default; the whole `container` test binary runs one case at a time (`src/.config/nextest.toml`).
- **Runner fix F2** (`src/decoder/src/connection.rs`): every reply was held up to the 50 ms read tick, so a probe took about 52 ms against a 50 ms budget; the loop now waits 5 ms for a frame while a job runs and restores the tick when it ends (about 15 lines; the 141 host tests stay green).

## Results (39 cases, run three times by the worker, and by the parent)

| Check | Result |
|---|---|
| The suite (parent): first run beside parallel cases on a host with a load average of 10 | 38 of 39: IT-10-3 "65,537 byte frame: the runner did not exit within 5 s" |
| After running the whole binary one case at a time (parent) | 39 of 39 in 95 s; worker: 39 of 39 in 56 to 65 s with two threads, three runs |
| fmt, clippy (workspace, and `decoder-runner --features test-freeze`), 141 host-process tests, pinning, script tests, `check-decoder-image.sh` | OK |
| `actionlint`, `cargo deny`, the `decoder-suite` CI job on a real runner | NOT RUN locally; CI is the proof |

Cases by id: AC-1; AC-2; IT-10-1; IT-10-2 (production container only); IT-10-3; IT-10-4 to IT-10-7 on the release image (JPEG, HEIC, PNG, WebP, GIF, a 30000x30000 header-only PNG refused in 40 ms, truncated JPEG, EXIF orientation 1 to 8 at 1440 and 320, the HEIC `irot` fixture, video posters, an HLG clip, six probe files); IT-10-12; IT-10-14; IT-10-15 (ptrace and `process_vm_readv` answer EPERM, `/proc/<runner>/mem` and `environ` EACCES, Seccomp 2, NoNewPrivs 1); IT-10-16; IT-10-17; IT-10-18 (the watchdog aborts 3.08 to 3.25 s after SIGUSR1, exit 134); IT-10-19 with stub markers; AC-7 (after every group no `work/`, scratch empty, only init and the runner). HLG poster SSIM (G-10-6, recorded, not asserted): 1.0000 against the harness chain, 0.9098 against no tone map.

**ffmpeg 9.1 and libvips 8.18.7 (owner decision: keep 9.1).** Compared with the host's own ffprobe, ffmpeg and libvips: video probe facts are identical for H.264, faststart, MOV, HLG and rotated clips; HLG poster SSIM 1.0000 against the host chain; orientation 1 to 8 and HEIC renders SSIM 1.0000. No difference found.

## Code Review Verdict

Not run: pending the batch review of the loop.

## NOT RUN (with the reason)

IT-10-2 self-check variants (five containers; `self_check` has three items, AZ-3861); IT-10-8 to IT-10-11 (transcode and remux return `Unsupported`, AZ-3860); IT-10-13 (needs the stall kill, AZ-3854 AC-6, the ffmpeg progress sink and transcode); IT-10-20 (the DS-16 red-team corpus, the 20 GB sparse video, the NVENC part: no `gen_redteam.py` here, needs AZ-3860 and AZ-3862); IT-10-17 character-device case (needs CAP_MKNOD); the real phone files DS-03 (100 JPEG, 20 HEIC), DS-05/06/08/35 and the 4K HLG (synthetic media used); PT-10-x, AT-10-x, ST-10-1 to 3; the video-marker (MP4 `free` box) path of the stub (implemented and unit-tested, no case uses it).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | F1: after a watchdog abort the job's `work/` survives; IT-10-18(b) says it is gone. `abort()` runs no destructors and AC-71 leaves scratch residue to the supervisor's start-up sweep. The case `known_failure_f1_it_10_18_*` fails with "work/ of the interrupted job survived the abort" and is excluded from the gate by `decoder-suite.sh` | IT-10-18, AC-71 | the owner says which side removes it (runner at abort, or the supervisor's start-up sweep with the test expecting that) | RESOLVED 2026-11-22 (owner): the supervisor's start-up sweep removes it. The known-failure case became `it_10_18_work_of_the_interrupted_job_is_left_for_the_supervisor_sweep_and_the_next_job_runs` (the runner leaves `work/`, the harness plays the sweep, the new instance serves the next job); `decoder-suite.sh` no longer excludes any case; tests.md IT-10-18 and the AC-71 row are amended (also the `docker exec ... kill -s` trigger, finding F3) |
| 2 | F3: `docker kill --signal=USR1` marks the container as stopped by hand, so `restart: unless-stopped` does not restart it; `tests.md` IT-10-18 names that command. The harness signals with `docker exec ... kill -s` | IT-10-18 | amend `tests.md` | clear |
| 3 | F4: the runner starts `vips thumbnail`, not `vipsthumbnail`; the stub replaces all five tools; 13.4 names four | decoder description 13.4 | amend the description | clear |
| 4 | F5: `hello` still returns "unknown" and build timestamp 1; IT-10-1's "non-empty" check passes on the placeholders | AZ-3861 | AZ-3861 reads `/usr/share/sunnycloud/build-facts.json` | clear |
| 5 | F6: `compose.dev.yml` gives the general decoder 2 CPU by default and the interactive scratch tmpfs `size=1g`; the docs say 4 CPU and 512 MiB. The suite uses the spec values | containerization.md | align the compose file or the docs | unclear |
| 6 | Scratch is a bind-mounted host directory (the interactive flavor uses `/dev/shm`, RAM-backed with no size cap), the runner runs as `10003:10003` with the host group added and `decoder_gid` is that group | AZ-3865 H3 setup | none; a tmpfs volume would need root for the harness | clear |
| 7 | `decoder_stall_kill=3s` is set but nothing enforces it until AZ-3854 AC-6 | AZ-3854 | build the stall kill (host-process slice) | clear |
| 8 | A 5 s exit bound failed once beside parallel cases on a loaded host; the whole container binary now runs one case at a time. On this shared host at a load average of 56 two IT-10-3 exit cases still missed 5 s (a 373 s suite run instead of 95 s): wall-clock bounds count only on a quiet host, as CI is | IT-10-3 | none | clear |

## Commit

`[AZ-3865] Decoder container suite H3: harness, test-stubs variant, 39 cases on the real image, CI job` — Loop: 1
