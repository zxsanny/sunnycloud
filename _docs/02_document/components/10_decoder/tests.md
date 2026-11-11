# Test Specification — 10 Decoder

Scope: the `decoder-runner` process in its container and its only interface, `decoder_protocol` v1 on the Unix socket (`hello`, `self_check`, `probe`, `render_still`, `transcode`, `cancel`, events `Alive` and `Progress`). 10 provides and requires no `contracts` ports. All tests use the real runner image with real libvips and ffmpeg and a protocol client built from `media-shared::decoder_protocol` (the harness is the "supervisor"; no 09 code is involved). System scenarios are cited by ID, not restated.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion | Test IDs | Coverage |
|-------|---------------------|----------|----------|
| AC-10 | WebP 320/1440, orientation swap 5-8, HEIC, SSIM >= 0.95, poster at supplied offset | IT-10-4, IT-10-5, IT-10-6, IT-10-7, PT-10-1; sys FT-P-20, FT-P-22, NFT-PERF-13 | Covered by this file and system scenarios; HLG poster SSIM >= 0.95 against the harness render of an equivalent chain (owner, 2026-11-11, G-10-6) |
| AC-11 | H.264 High 1080p AAC MP4 faststart, lossless remux, 4K HLG tone map | IT-10-8, IT-10-9, IT-10-10, IT-10-11, PT-10-2; sys FT-P-23, FT-P-24, FT-P-25, NFT-PERF-15, NFT-PERF-16 | Covered by this file (correctness on short clips) and system scenarios (speed budgets, `C-REF`) |
| AC-25 | Whitelists, `VIPS_BLOCK_UNTRUSTED`, limits, hostile corpus -> failure, self-check, containment | IT-10-2, IT-10-12, IT-10-13, IT-10-14, IT-10-15, IT-10-16, IT-10-17, IT-10-18, IT-10-19, ST-10-1, ST-10-2, ST-10-3, ST-10-4; sys FT-N-07, NFT-SEC-28, NFT-SEC-29, NFT-SEC-45, NFT-RES-LIM-04 | Covered by this file and system scenarios; IT-10-18 (watchdog) runs with the `test-freeze` trigger (AZ-3851) |
| AC-28 | NVENC probe by test encode, exit-code reporting (v1.1) | IT-10-20; sys FT-N-08, FT-P-28, NFT-PERF-18 | Covered for the no-GPU path; GPU path is sys-only (H-GPU, v1.1) |
| AC-44 | `maxrate`/CRF keep derived at most 20% of original | IT-10-8, PT-10-2; sys FT-P-26, NFT-PERF-20 | Covered by this file (maxrate/compliance of output) and system scenarios (ratio on 100 clips) |
| AC-45 | Kill within 90 s of last progress; orientation and A/V drift | IT-10-9, IT-10-13, PT-10-3; sys FT-N-06, FT-P-27, NFT-PERF-21 | Covered by this file and system scenarios |
| AC-71 | No state between jobs; `work/` removed on every path; socket loss = cancel | IT-10-14, IT-10-15, IT-10-19; sys NFT-RES-36, NFT-SEC-45 | Covered by this file and system scenarios |
| AC-72 | The runner trusts only `in/source` that 09 verified | none at this boundary | Not independently testable: the decoder does not verify hashes by design; verification is 09 IT-09-7 and sys FT-N-43 |

## Blackbox Tests

Harness `H3`: the real `decoder-general` and `decoder-interactive` images started by Compose from the test stack (`network_mode: none`, read-only rootfs, cgroup limits as production), the socket volume mounted into the harness container. A "test image variant" of the same image adds only a stub layer for tests marked `stub`: `ffmpeg`, `ffprobe`, `vipsthumbnail` and `vipsheader` are replaced by one compiled stub binary that behaves according to a marker inside `in/source` (hang, allocate, ignore signals, `setsid` plus double fork, write to the next job's path, call `ptrace`, 1 MiB of stderr) and otherwise logs its argv and environment and runs the real tool. The stub layer is built from the same Dockerfile with `--target test-stubs` and `FEATURES=test-freeze`; mechanism, marker format and release-image check are decided (AZ-3850, G-10-1; description section 13.4). Every test marked `stub` uses it; tests without the mark run against the real tools of the same image. Test config: `decoder_stall_kill=3s`, `decoder_alive_interval=500ms`, `decoder_watchdog=3s`, `decoder_memory_poll_interval=100ms`, `photo_max_pixels` as the case states; the 90 s default is used once (IT-10-13b).

### IT-10-1: Handshake
**Summary**: `hello` returns capabilities; a wrong protocol version is refused.
**Traces to**: AC-25, AC-45
**Input data**: `hello` with protocol 1; with protocol 99; `hello` repeated 3 times on the same connection.
**Expected result**: `Capabilities{protocol: 1, vips, ffmpeg, libheif versions non-empty, build_timestamp (RFC 3339 or epoch, not in the future), nvenc_probe_ok: false, nvenc_sessions: 0}`; protocol 99 gives `ProtocolMismatch` and the runner stays up (clean error, not hostile-class); repeated `hello` is answered each time.
**Max execution time**: 5 s
**Dependencies**: decoder container

### IT-10-2: Self-check reports facts
**Summary**: `self_check` reports each isolation property as data.
**Traces to**: AC-25
**Input data**: `forbidden_targets = [api:8080, api:8081, db:5432, 1.1.1.1:53, example.com:443]`; once on the production container; once on a variant started with `cap_add: NET_RAW`, once as uid 0, once with a writable rootfs, once with an extra env var `X_TEST=1`, once with the blob path bind-mounted.
**Expected result**: production: every item `ok` (no route, DNS fails, environ only the allowlist, mountinfo without blob path or secrets, uid != 0, `CapEff == 0`, `no_new_privs`, rootfs read-only); each variant flips exactly its own item to `ok = false` and leaves the others ok; the call itself never errors; names are stable strings.
**Max execution time**: 30 s (6 container starts)
**Dependencies**: decoder container variants

### IT-10-3: Framing and busy handling
**Summary**: length-prefixed JSON, 64 KiB cap, one request in flight.
**Traces to**: AC-25
**Input data**: a second `render_still` while one runs; a frame of 65,537 bytes; a frame with an unknown field; a length prefix of 4 GiB; `limits` with `cpu = 9999`, `memory_bytes = 1 TiB`, `wall = 1 day`.
**Expected result**: second run answers `Busy` and the first job completes normally; oversize, unknown-field and absurd-length frames are protocol violations: reply `ProtocolViolation` (or close) and the runner exits within 5 s (hostile-class); oversized `limits` are clamped to the ceilings (checked in IT-10-12).
**Max execution time**: 30 s
**Dependencies**: decoder container

### IT-10-4: Photo probe is a header read
**Summary**: `probe` on photos answers dimensions without decoding.
**Traces to**: AC-10
**Input data**: DS-03 JPEG 12 MP, HEIC 12 MP, PNG, WebP, GIF, a 30000x30000 PNG (DS-16), a truncated JPEG.
**Expected result**: `Probe{kind: photo, width, height}` equal to the manifest; latency per probe <= 50 ms for the 12 MP files and <= 100 ms for the huge PNG; runner RSS delta under 50 MiB (no decode); truncated file `Corrupt` (or valid dimensions if the header is intact, recorded).
**Max execution time**: 10 s
**Dependencies**: decoder container, DS-03, DS-16

### IT-10-5: Thumbnails and preview
**Summary**: `render_still` outputs WebP with orientation applied and dimensions read back.
**Traces to**: AC-10
**Input data**: DS-03 JPEGs with EXIF orientation 1 to 8 (generated for 2, 4, 5, 7 in addition to the 3, 6, 8 present), 12 MP; `max_edge` 320 and 1440; 20 HEIC.
**Expected result**: `out/result.webp` is a regular WebP, `ImageResult{width,height,bytes}` equals the file's real dimensions; long edge equals `max_edge`; for orientations 5 to 8 the width/height are swapped relative to the stored pixels; SSIM >= 0.95 against the harness reference render (own libvips auto-rotate, same size) for every file; no EXIF/GPS/ICC strings in the output (`exiftool` shows none); no `--linear`/unlimited flags in the argv log (stub run).
**Max execution time**: 120 s for the set
**Dependencies**: decoder container, DS-03

### IT-10-6: Pixel limit before decode
**Summary**: `photo_max_pixels` is enforced from the header.
**Traces to**: AC-25, AC-10
**Input data**: runner started with `photo_max_pixels=1000000`: 1000x1000 PNG and 1001x1000 PNG; default config: DS-16 30000x30000 PNG.
**Expected result**: 1000x1000 renders; 1001x1000 gives `LimitExceeded{pixels}` in under 1 s; 30000x30000 gives `LimitExceeded{pixels}` in under 2 s with runner RSS never above 200 MiB; the runner exits after the reply (hostile-class) and the container is restarted.
**Max execution time**: 60 s
**Dependencies**: decoder container, DS-16

### IT-10-7: Video poster frame
**Summary**: `render_still` on a video decodes the frame at `poster_at_ms`; the HLG poster part records SSIM only.
**Traces to**: AC-10
**Input data**: DS-08 clips with flash frames at known timestamps; `poster_at_ms` 1000 and 900; a 4K HLG clip (DS-06 base).
**Expected result**: the poster is the frame nearest the requested time (the flash marker color/position matches the manifest for the timestamp, tolerance one frame); `poster_at_ms` is ignored for images; output goes through Profile A to WebP of 320 px. The HLG part asserts SSIM >= 0.95 of the 320 px poster against the harness render of an equivalent tone-mapping chain (the reference and the tolerance were decided by the owner on 2026-11-11, G-10-6; the same 0.95 as AC-10) and prints the value in the report. Measured on a SYNTHETIC HLG clip: 0.995 or more (description section 13.3); the real DS-06 base and REF-CPU4 are still to be run.
**Max execution time**: 60 s
**Dependencies**: decoder container, DS-08, DS-06

### IT-10-8: CPU transcode output
**Summary**: `Plan::Transcode` produces a compliant 1080p H.264/AAC MP4 with faststart.
**Traces to**: AC-11, AC-44
**Input data**: DS-05 base (1080p 18-20 Mbit/s); a 4K SDR clip; a portrait 2160x3840 clip; a clip with GPS/title metadata and subtitle stream; `Plan{encoder Cpu, scale_to_short_edge 1080, tone_map false, audio aac, faststart true}`.
**Expected result**: `TranscodeResult.probe_of_output` equals an independent `ffprobe` of `out/result.mp4`; H.264 High, `yuv420p`, BT.709 tags, video bitrate <= 10 Mbit/s (average and worst 1 s window), AAC 128 kb/s stereo, `moov` before `mdat`; sizes: 1080p stays 1920x1080, 4K becomes 1920x1080, portrait becomes 1080x1920 (long <= 1920, short <= 1080), even dimensions; only the first video and first audio stream, no subtitles, no GPS/title metadata; duration within 1 s of input; output smaller than input for DS-05 (size ratio recorded).
**Max execution time**: 300 s on `C`; speed budgets are `C-REF`
**Dependencies**: decoder container, DS-05, generated variants

### IT-10-9: Orientation, VFR and A/V sync
**Summary**: rotated and variable-frame-rate clips come out right.
**Traces to**: AC-11, AC-45
**Input data**: DS-08 clips rotated by display matrix 90/180/270 (3 clips), 3 VFR clips with beeps at known timestamps.
**Expected result**: the displayed orientation of the output matches the flash-frame marker position in the manifest (no rotation metadata left that would double-rotate); audio beep onsets are within 40 ms of the manifest timestamps; output has no `-noautorotate` effect (stub argv log shows autorotate default).
**Max execution time**: 300 s
**Dependencies**: decoder container, DS-08

### IT-10-10: HDR to SDR
**Summary**: HLG input is tone-mapped to SDR BT.709.
**Traces to**: AC-11
**Input data**: DS-06 base clip (4K HEVC Main10 HLG, 30 s), `Plan{tone_map true, scale_to_short_edge 1080}`.
**Expected result**: output 1920x1080 `yuv420p`, `color_primaries/transfer/space = bt709`, H.264 High; the output probe passes `is_compliant`; mean luma (8-bit code values, `signalstats` YAVG over the clip) of the output is within **2.0** of the harness render, which the harness makes with its own ffmpeg using the chain recorded in description section 13.2 on the same base file (proposed from SYNTHETIC measurements: encode noise 0.2, order variants 0.06; confirm on the real DS-06 base at gate S5 on REF-CPU4); a missing tone map (mean luma 60 code values higher) and a wrong order or operator are caught. The tags are asserted exactly. The SDR source of the generator is not the reference: tone mapping changes luma by design (about 81 code values on the synthetic clip).
**Max execution time**: 600 s on `C`
**Dependencies**: decoder container, DS-06

### IT-10-11: Lossless remux
**Summary**: `Plan::Remux` copies streams and moves the index to the front.
**Traces to**: AC-11, AC-44
**Input data**: DS-35 moov-at-end clip (compliant codecs) and a variant with an extra subtitle stream and chapters.
**Expected result**: `ffprobe -show_packets` MD5 of the video and audio packet payloads equals the input's; codec, profile, bitrate and frame count unchanged; `moov` before `mdat`; subtitle, data streams, chapters and global metadata dropped; no re-encode (encoder tag unchanged, wall time <= 20% of a transcode of the same clip); output probe `moov_first = true`.
**Max execution time**: 60 s
**Dependencies**: decoder container, DS-35

### IT-10-12: Limits and clamping
**Summary**: requested limits are clamped to the runner's ceilings and the guard enforces them.
**Traces to**: AC-25
**Input data**: runner ceilings `photo_job_timeout=3s`, `photo_memory=128MiB`; requests ask for `wall=3600s`, `memory_bytes=64GiB`, `cpu=64`, `max_output_bytes=1TiB`; `stub` markers: hang, allocate 512 MiB, write 1 GiB output, spin on all cores; also a real photo job to show normal work still passes.
**Expected result**: hang killed at <= 3 s + 1 s with `Killed{wall}` -> `LimitExceeded{wall}`; allocation killed within 1 s of the 128 MiB threshold (poll 100 ms) -> `LimitExceeded{memory}`; output write stopped by `RLIMIT_FSIZE` -> `LimitExceeded{output}`; thread counts in the argv/env log equal the (clamped) cpu limit (`VIPS_CONCURRENCY`, `-threads`, `-filter_threads`); no `RLIMIT_AS` in `/proc/<pid>/limits`; RLIMIT_CORE 0.
**Max execution time**: 60 s
**Dependencies**: decoder test-stub image

### IT-10-13: Stall watchdog
**Summary**: ffmpeg without a progress line is killed after `stall_timeout` and reported `Stalled`.
**Traces to**: AC-45
**Input data**: (a) real ffmpeg transcode of DS-05 base with `decoder_stall_kill=3s` and ctl `SIGSTOP` of the ffmpeg PID; (b) the same with the default 90 s: stop ffmpeg and time from its last `Progress` event.
**Expected result**: (a) `Stalled` within 3 s + 1 s of the last progress line, ffmpeg gone (no process in the container), `work/` removed, runner exits (hostile-class) and restarts; (b) `Stalled` within 90 s + 2 s after the last progress line (not before 88 s); no `Stalled` while progress lines keep arriving.
**Max execution time**: (a) 60 s, (b) 150 s
**Dependencies**: decoder container, ctl, DS-05

### IT-10-14: Containment — descendants and the next job
**Summary**: a compromised child cannot outlive its job or read the next job's input.
**Traces to**: AC-25, AC-71
**Input data**: stub ffmpeg that (1) calls `setsid`, double forks, then loops trying to open `/scratch/<next job id>/in/source` and appends `LEAK` to `/scratch/leak.txt` on success; (2) ignores SIGTERM; job A with that marker, then job B (a normal photo) sent immediately after A's reply.
**Expected result**: A's reply arrives only after kill-all; no process descended from the runner exists when B is accepted (`ctl` lists container processes); `/scratch/leak.txt` absent or without `LEAK` after B; B succeeds; because a descendant was found at the end of A the outcome is hostile-class (the description treats a surviving descendant as hostile; whether a descendant that is found and then killed counts is ambiguous, gap G-10-2): the runner exits after A's reply, the container restarts, and B runs on the new instance.
**Max execution time**: 60 s
**Dependencies**: decoder test-stub image, ctl

### IT-10-15: Containment — ptrace, process_vm and /proc
**Summary**: seccomp and `PR_SET_DUMPABLE=0` stop peers from reading the runner.
**Traces to**: AC-25
**Input data**: stub child as the same uid: `ptrace(PTRACE_ATTACH, runner_pid)`, `process_vm_readv(runner_pid)`, `open("/proc/<runner>/mem")`, `open("/proc/<runner>/environ")`, `ptrace(PTRACE_TRACEME)`.
**Expected result**: each returns `EPERM` or `EACCES` (not `ENOSYS`, which would mean the filter is absent: the seccomp profile must deny, not remove, the calls); the stub logs errno values to its `out/`; `/proc/<runner>/status` shows `Seccomp: 2`, `NoNewPrivs: 1`; the runner is a child subreaper (`/proc/<runner>/status` has no direct field; checked by the orphan test in IT-10-14).
**Max execution time**: 20 s
**Dependencies**: decoder test-stub image

### IT-10-16: Hostile-class outcomes end the runner; clean failures do not
**Summary**: the outcome classes decide whether the runner exits.
**Traces to**: AC-25
**Input data**: stub child killed by SIGSEGV; `Killed{memory}`; `LimitExceeded`; peer protocol violation; versus truncated JPEG (`Corrupt`), SVG named `.png` (`Unsupported` or `Corrupt`), a clean non-zero exit.
**Expected result**: hostile-class: typed reply first, then the process exits within 5 s and Compose restarts it (container start time changes, `hello` answers again within 30 s); clean `Corrupt`/`Unsupported`: the same container keeps serving (start time unchanged), next job runs; `work/` is removed on all paths.
**Max execution time**: 120 s
**Dependencies**: decoder test-stub image, ctl

### IT-10-17: Input safe-open
**Summary**: `in/source` and `job_dir` are opened without following links and only as regular files.
**Traces to**: AC-25
**Input data**: `in/source` as FIFO, symlink to `/etc/passwd`, hardlink (`nlink = 2`), directory, character device; `job_dir` as `../x`, a symlink inside `scratch_root`, an absolute path outside it.
**Expected result**: every case answers `Corrupt` (or protocol error for the path cases) within 1 s with no blocking on the FIFO; no byte of `/etc/passwd` appears in any output or log; a valid control job runs after each.
**Max execution time**: 30 s
**Dependencies**: decoder container

### IT-10-18: Self-abort watchdog
**Summary**: a frozen main loop makes the runner abort within `decoder_watchdog`.
**Traces to**: AC-25
**Input data**: the `test-stubs` image (built with `FEATURES=test-freeze`); (a) idle runner; (b) runner with a running job (stub marker `hang`, so the main loop is waiting on the guard); the harness sends `SIGUSR1` with `docker kill --signal=USR1` (the `init: true` process forwards it to the runner); control: the release image receives no signal in this test.
**Expected result**: in (a) and (b) the runner logs `test_freeze_main_loop: main loop blocked`, the watchdog thread keeps running and the process aborts (SIGABRT, exit 134) within `decoder_watchdog` (3 s in test config) + 1 s of the last counter bump (about 3.0 to 3.2 s after the signal; a core-dump handler on the host may add up to 1 s, so the runner sets its own `RLIMIT_CORE=0`), the container restarts, `hello` answers on the new instance within 30 s, `work/` of (b) is gone. Contrast check in the same test: `SIGSTOP` of the runner does not abort it (all threads stop), which is why the trigger is not `SIGSTOP`. Release-image check (CI, AZ-3851): `strings` and `nm` of the release `decoder-runner` show no `test_freeze_main_loop` and no `test_freeze` symbol.
**Max execution time**: 20 s
**Dependencies**: decoder test image (`test-freeze`, description section 3 "Self-abort watchdog")

### IT-10-19: Socket loss and cancel
**Summary**: closing the connection mid-job acts as `cancel`; `cancel` kills the group and removes `work/`.
**Traces to**: AC-71, AC-45
**Input data**: real transcode of DS-05 base; (a) explicit `cancel` at 2 s; (b) the harness closes the socket at 2 s; (c) the harness reconnects after (b).
**Expected result**: (a) `Cancelled` within 2 s, ffmpeg and its process group gone, `work/` removed, `out/` partial files may remain (09 owns scratch removal); (b) the same without a reply, CPU of the container back to idle within 2 s; (c) the runner accepts a new connection and a new job without restart (socket loss is not hostile-class).
**Max execution time**: 60 s
**Dependencies**: decoder container, DS-05

### IT-10-20: Red-team corpus and NVENC path
**Summary**: every DS-16 item ends in a typed failure and the next job runs; the no-GPU encoder path is clean.
**Traces to**: AC-25, AC-28
**Input data**: DS-16: SVG and PDF renamed `.png`, `.m3u8` referencing `file:///etc/passwd` and `/proc/self/environ` named `.mp4`, a concat file, 30000x30000 PNG, 20 GB sparse video, truncated HEIC, zip-bomb metadata; control image and control video; `nvenc=on` on a host without GPU.
**Expected result**: each hostile item returns a `DecodeError` (`Unsupported`, `Corrupt`, `LimitExceeded{...}`) with no success; no output file and no log line contains `root:x:0:0` or environment contents; the 20 GB video ends within its limits (wall/output) and the runner restarts if hostile-class; the controls then succeed; `nvenc=on` without GPU: `hello.nvenc_probe_ok = false`, the argv log shows no `h264_nvenc` use, and a `Plan.encoder = Nvenc` request answers `EncoderFailed` or is rejected without a crash [v1.1].
**Max execution time**: 20 min (the 20 GB file crosses the volume; `blobs-large` not needed, scratch only)
**Dependencies**: decoder container, DS-16

## Performance Tests

### PT-10-1: Photo render latency
**Summary**: 12 MP JPEG/HEIC render times, warm runner.
**Traces to**: AC-10
**Load scenario**:
- 1 slot, sequential jobs: 100 JPEG then 20 HEIC (DS-03), edges 1440 then 320, interactive decoder limits (2 CPU)

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| JPEG preview+thumb pair p95 | 600 ms | 2 s |
| HEIC preview+thumb pair p95 | 1.5 s | 3 s |
| Probe p95 | 30 ms | 100 ms |

**Resource limits**:
- CPU 2, memory 2 GiB (cgroup). Targets proposed; the 5 s end-to-end bound is sys NFT-PERF-13 (gate S4, HEIC risk).

### PT-10-2: Transcode speed on the reference worker
**Summary**: transcode of short clips is measured; budgets are sys.
**Traces to**: AC-11, AC-44
**Load scenario**:
- DS-05 base (30 s) and DS-06 base (30 s) on REF-CPU4, one job at a time

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| 1080p wall time per second of media | 0.5 s/s | 0.5 s/s (10-min clip must finish in 5 min) |
| 4K HLG wall time per second of media | 2.0 s/s | 2.0 s/s (20 min for 10 min) |
| Output/input ratio, DS-05 | 20% or less | 35% |

**Resource limits**:
- 4 physical cores, 4 GiB. Valid only on REF-CPU4; elsewhere `NOT-RUN(HW-REF)`. Full 10-minute budgets: sys NFT-PERF-15/16.

### PT-10-3: Runner idle and alive cost
**Summary**: an idle runner stays quiet and emits `Alive` on time.
**Traces to**: AC-45
**Load scenario**:
- Runner idle for 10 minutes with `decoder_alive_interval=5s`; during a 60 s transcode, `Alive` events observed

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Idle CPU | under 0.5% | 2% |
| `Alive` interval during a job | 5 s +/- 0.5 s | 6 s |
| Memory growth over 100 jobs | under 20 MiB | 100 MiB |

**Resource limits**:
- Container limits as production.

## Security Tests

### ST-10-1: No credentials, network or blob access
**Summary**: the container has no network, secrets or blob mount.
**Traces to**: AC-25
**Attack vector**: a compromised decoder reaching the API, DB or blob volume.
**Test procedure**: `ctl inspect` / `nsenter` into both containers: `ip route`, `/etc/resolv.conf`, mounts, env, `CapEff`, cgroup limits; from a stub child try `connect` to `api:8081`, `db:5432`, an external IP, and open `/var/lib/sunnycloud/blobs`.
**Expected behavior**: no interface except loopback; every connect fails; no blob path; no secrets; limits equal 2 CPU/2 GiB (interactive) and 4 CPU/4 GiB (general), `pids_limit` set.
**Pass criteria**: all hold.
**Fail criteria**: any route, secret or mount found.

### ST-10-2: Argument and environment hygiene
**Summary**: children get a fixed argv, no shell, a cleared environment.
**Traces to**: AC-25
**Attack vector**: injection through crafted file names or `Plan` values.
**Test procedure**: stub argv/env log for photo and video jobs with `job_dir` names containing spaces, `;`, `$(x)`, quotes; send a `Plan` with a free-form field and a `max_edge` of -1 / 0 / 100000.
**Expected behavior**: argv contains `-nostdin`, `-protocol_whitelist file`, `-format_whitelist`, `-codec_whitelist`, `-map_metadata -1`, first video/audio mapping only; no `/bin/sh` process is created; env is exactly the allowlist (`VIPS_BLOCK_UNTRUSTED=1` for Profile A, unset for HEIC Profile B); malformed plan or edge is `ProtocolViolation`.
**Pass criteria**: golden argv files match; 0 shell invocations.
**Fail criteria**: any deviation.

### ST-10-3: Hostile-type matrix
**Summary**: sniffing chooses the profile and unsupported containers never reach a loader.
**Traces to**: AC-25
**Attack vector**: polyglots and renamed files (JPEG+HTML polyglot, GIF89a/HTML, MP4 with `ftyp` heic brand, EBML file with HLS text).
**Test procedure**: one job per file; log which binary and profile ran.
**Expected behavior**: JPEG/PNG/WebP/GIF -> Profile A; HEIC/AVIF brands -> Profile B; video containers -> poster path; anything else `Unsupported`; SVG/PDF/Magick loaders absent (`vips -l` list has none).
**Pass criteria**: the sniff table (7 inputs) matches expected profile for each.
**Fail criteria**: a mismatched profile or an SVG/PDF load.

### ST-10-4: Information disclosure in logs and errors
**Summary**: logs and errors carry no user paths or large stderr.
**Traces to**: AC-25
**Attack vector**: log injection and leakage via decoder stderr.
**Test procedure**: force a kill with a stub writing 1 MiB of stderr containing a fake secret and ANSI/newline sequences.
**Expected behavior**: log holds at most a 2 KiB sanitised tail, no control characters, no job directory paths of user content (`job_id` only).
**Pass criteria**: tail length <= 2,048 and sanitised.
**Fail criteria**: unsanitised or longer.

## Acceptance Tests

### AT-10-1: Corpus run, then normal service
**Summary**: component acceptance for hostile input.
**Traces to**: AC-25
**Preconditions**:
- Both decoders running; DS-16 and DS-03 available.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Send every DS-16 item as a job | typed failure each time, no data exposure |
| 2 | Send the control image and control video | both succeed |
| 3 | Inspect containers | no extra processes, scratch empty, containers healthy |

### AT-10-2: Correct outputs and kill timing
**Summary**: orientation, remux and stall behaviour as acceptance.
**Traces to**: AC-11, AC-45
**Preconditions**:
- DS-08 clips, DS-35.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Transcode rotated and VFR clips | orientation correct, drift <= 40 ms |
| 2 | Remux the moov-at-end clip | packets identical, index first |
| 3 | Stop ffmpeg mid-transcode | `Stalled` within 90 s of the last progress line; CPU rerun is 09's (sys FT-N-06) |

## Test Data Management

**Required test data**:

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-03 | 100 JPEG, 20 HEIC, orientations | `gen_photos.py` plus generated orientations 2, 4, 5, 7 | about 450 MB |
| DS-05 base, DS-06 base | 30 s base clips (1080p SDR, 4K HLG) | `gen_video.py` before looping; the base files are kept as artifacts with the manifest entries of description section 13.5 (G-10-3, requirement on E-31/c21) | about 70 MB, 150 MB (DS-06 3 GB / 20 loops) |
| DS-07, DS-35 | compliant, edge, control clips | `gen_video.py` | about 300 MB |
| DS-08 subset | rotated, VFR, flash-frame and beep clips | `gen_video.py mixed` | about 800 MB |
| DS-16 | red-team media | `gen_redteam.py` | 20 GB apparent, 30 MB allocated |
| stub markers | small files selecting stub behaviour | written with the tests | < 1 MB |

**Setup procedure**:
1. Build or pull the decoder image (and the `test-stubs` variant) by digest; start the container with production flags (`read_only`, `cap_drop ALL`, `no-new-privileges`, seccomp, `network_mode: none`, `init: true`, `restart: unless-stopped`).
2. The harness creates `/scratch/<job id>/in/source` with mode 0440 and group `decoder_gid`, then sends the request; reference outputs (SSIM, packet hashes) come from the harness's own libvips/ffprobe, never from the runner's report.

**Teardown procedure**:
1. Remove job trees, restart the container if hostile-class, check the scratch volume is empty and no stray process remains.

**Data isolation strategy**: a unique job directory per test on a per-test scratch volume (tmpfs for the interactive decoder, disk for the general one); the container is recreated between groups that end hostile-class; tests share no files. Real vips and ffmpeg run everywhere; the only substitutes are the stub binaries of the `test-stubs` variant, used solely to create hostile behaviour that real tools do not produce on demand.

## Spec gaps found

- G-10-1: RESOLVED 2026-11-11 (AZ-3850, agent decision, owner may overturn): `test-stubs` Dockerfile target with one compiled stub binary, no runtime config key; description section 13.4. E-19/c15 builds it.
- G-10-2: whether a descendant that the kill-all walk finds alive (and kills) counts as a hostile-class outcome is ambiguous ("a descendant found alive at this point").
- G-10-3: DS-05/06 are generated as 10-minute loops; tests need the 30 s base files exposed in the manifest. Requirement recorded (AZ-3850, description section 13.5); stays open until E-31/c21 ships the files.
- G-10-4: RESOLVED 2026-11-11 (AZ-3851, agent decision, owner may overturn): feature `test-freeze` with a `SIGUSR1` handler that blocks only the main loop; description section 3 and 13.6; E-19/c07b implements, E-19/c15 and IT-10-18 use it.
- G-10-5: SVG named `.png`: step 1 of the image profile says `Unsupported` for unsniffable input, while the same paragraph says it fails as `Corrupt`; tests accept either typed code.
- G-10-6 (resolved 2026-11-11 by the owner: SSIM >= 0.95 against the harness render of an equivalent chain; formerly record-only): poster-frame HDR tolerance was an open owner item; measured SSIM values are recorded (AZ-3851, description section 13.3), the tolerance is still the owner's. Reference decided 2026-11-11 (owner): the harness render of an equivalent chain; tolerance still to be confirmed (recommended 0.95, as AC-10).
- G-10-7: the reply type for a peer protocol violation is not in the `DecodeError` list (tests accept a typed error or a closed socket, then require the exit).
