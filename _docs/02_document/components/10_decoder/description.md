# 10 Decoder

## 1. High-Level Overview

**Purpose**: The untrusted `decoder-runner` process in its own container (no credentials, no blob mount; `network_mode: none`, the socket volume is its only channel). It turns one decrypted scratch file into thumbnails, a preview, a poster frame, a remux or a transcode by running `vipsthumbnail` or `ffmpeg` as child processes under hard limits, and reports a typed result. One responsibility: *probe and execute a single, fully specified media job safely and report the outcome; make no decisions about policy*. The transcode policy (compliance, remux, poster offset) is applied by the supervisor (09) from `media-shared`.

**Architectural pattern**: Single-job request loop on a Unix socket (server side of `media-shared::decoder_protocol`) over two profile-based executors (`ImageExecutor`, `VideoExecutor`) and one `ProcessGuard` that applies limits and kills the process group.

**Crate dependencies**: `media-shared` (types `Probe`, `Plan`, `Params`, `OutputSpec`; the `decoder_protocol` module, owned here: request/response/event types, linked by 09), helpers 01 config (typed env section) and 02 error (error code enum only). Nothing else: no clock helper (monotonic `std::time` for timers), no database (S4-1), no DB, no HTTP client, no `contracts` port, and it never calls `media-shared::policy`.
**Downstream consumers**: 09_worker_supervisor (only client of the socket).

**Scope in**: protocol server; magic-byte sniffing and profile choice; image profiles (A, B-HEIC); ffmpeg/ffprobe invocation with whitelists; MP4 atom-order check; lossless remux; HDR to SDR tone mapping; CPU x264; opt-in NVENC probe by test encode [v1.1]; stall watchdog and per-kind limits; output validation; self-check; image build facts (versions, build timestamp).
**Scope out**: choosing what to do with a probe (the decoder only probes and executes the plan it is given), compliance and poster-frame rules (09 with `media-shared::policy`), fetching/posting bytes, job state, retries beyond reporting, encryption of outputs, the image build recipe (deployment docs).

## 2. Ports

10 provides and requires no `contracts` ports. Its only interface is the decoder protocol (wire contract in `media-shared::decoder_protocol`, server side here, client side in 09). There are two decoder containers per supervisor: `decoder-interactive` (image jobs only) and `decoder-general` (video jobs and image jobs); each runs one runner with its own socket. Jobs planned as video never reach the interactive decoder (its scratch is a 512 MiB tmpfs); a video container inside a photo job is bounded by the 256 MiB photo input limit.

## 3. Internal Interfaces

### DecoderProtocol v1 (Unix socket in a volume shared with the supervisor only)
Framing: u32 length prefix, JSON, max 64 KiB, one request in flight; a second `run` while busy answers `Busy`. Connection loss while a job runs is treated as `cancel` (a dead supervisor must not leave a CPU-burning decoder). The runner serves one connection at a time and exits after a hostile-class outcome (section 3, Containment).

| Request | Input | Response | Errors (`DecodeError`) |
|---|---|---|---|
| `hello` | protocol version | `Capabilities{protocol, vips, ffmpeg, libheif versions, build_timestamp, nvenc_probe_ok, nvenc_sessions}` | `ProtocolMismatch` |
| `self_check` | `forbidden_targets[]` (host:port to attempt) | `SelfCheckReport[{name, ok}]` | none (failed items are data) |
| `probe` | `job_dir` | `Probe{kind photo\|video, container, moov_first, video{codec, profile, width, height, rotation, bitrate, pix_fmt, primaries, transfer, matrix}, audio{codec}, duration_ms}` (photos fill `width`/`height` only) | `Unsupported`, `Corrupt`, `LimitExceeded` |
| `render_still` | `job_dir`, `max_edge`, `poster_at_ms` (used for video containers, ignored for images), `limits` | `ImageResult{width, height, bytes}`; file at `out/result.webp` (photo thumbnail or preview; for a video container, a poster frame) | `Unsupported`, `Corrupt`, `LimitExceeded{wall, memory, pixels, output}`, `Busy` |
| `transcode` | `job_dir`, `Plan` (`Remux` or `Transcode{encoder, scale_to_short_edge, tone_map, audio, faststart}`), `limits` | `TranscodeResult{probe_of_output, bytes}`; file at `out/result.mp4` | above plus `Stalled`, `EncoderFailed{exit_code}`, `OutputInvalid` |
| `cancel` | none | `Cancelled` (process group SIGKILLed, `work/` removed) | none |

`DecodeError` also has `ProtocolViolation` (malformed or oversize frame, unknown field, request out of sequence; hostile-class, any request).

Events while a job runs: `Alive` every `alive_interval` (5 s), `Progress{out_time_ms, speed}` for each ffmpeg progress line. `limits` = `{cpu, memory_bytes, wall, stall_timeout, max_output_bytes}`; each value is clamped to the runner's own ceiling config, so a compromised supervisor cannot raise them. `job_dir` must be inside `scratch_root`; the runner resolves it with no symlinks, reads only `in/source`, writes only `out/` and `work/`. Before handing `in/source` to vips or ffmpeg it opens it with `O_PATH | O_NOFOLLOW`, `fstat`s that it is a regular file with `st_nlink == 1`, and reopens it through `/proc/self/fd/N` with `O_NONBLOCK`; anything else is `Corrupt`.

### Process guard (internal)
Starts a child with an argv vector (never a shell), `env_clear()` plus a fixed allowlist, `-nostdin`, its own process group, `RLIMIT_CORE=0`, `RLIMIT_NOFILE`, `RLIMIT_FSIZE = max_output_bytes`, `RLIMIT_CPU` as backstop; polls group RSS every second and kills above the memory limit; kills on wall limit; for ffmpeg kills when no progress line for `stall_timeout`. Returns `Exited{code}` or `Killed{reason: wall|memory|stall}`; fallback keys on exit codes, never message text. **No `RLIMIT_AS`** (breaks NVENC below 4 GiB, fact card). Thread caps approximate CPU limits: `VIPS_CONCURRENCY`, `-threads`, `-filter_threads` = limit cpu.

### Containment (decoders are assumed compromised)
- At start the runner sets `PR_SET_DUMPABLE=0` (no `ptrace`/`/proc/<pid>/mem` access to it or its children from a peer process) and `PR_SET_CHILD_SUBREAPER`, so any orphaned or double-forked descendant of vips/ffmpeg reparents to the runner. The container seccomp profile denies `ptrace` and `process_vm_readv/writev`, with `no-new-privileges`.
- **After every job, on every exit path**, the runner SIGKILLs *all* descendants, not only the process group: it walks `/proc` for every process whose ancestry reaches the runner (`setsid` and double-fork do not escape the walk), kills, reaps, and repeats until none is left. It then removes `work/`. A descendant found alive and killed at this point is itself a hostile-class outcome (the runner then exits, see below).
- **Hostile-class outcomes** are: a child killed by a signal, `Killed{wall|memory|stall}`, `LimitExceeded`, a surviving descendant, a protocol violation by the peer. After sending the typed reply the runner exits; Compose restarts the container (`restart: unless-stopped`) with clean memory, and the supervisor reconnects and repeats `hello` and `self_check`. `Corrupt` and `Unsupported` from a clean non-zero exit are not hostile-class (truncated uploads are common).
- **Self-abort watchdog**: Docker does not restart a container whose healthcheck fails, so the runner polices itself. The main loop bumps a monotonic counter at least every `alive_interval`; a watchdog thread calls `abort()` when the counter has not moved for `decoder_watchdog` (default 20 s, i.e. four intervals) whether a job is running or idle. The container exits and is restarted.
- One job at a time and a wiped tree per job mean a compromised child can neither outlive its job nor read the next job's input (the next `in/source` is created after the kill-all and only then does the runner accept another request). This is a hardening of a shared-kernel container, not a VM boundary; no gVisor in v1.0.

## 4. Execution profiles

### Image (libvips 8.18.x, CLI only, never the crate)
1. Sniff magic bytes in the runner: JPEG, PNG, WebP, GIF -> **Profile A** (`VIPS_BLOCK_UNTRUSTED=1`); HEIC/HEIF/AVIF (`ftyp` brands) -> **Profile B** (variable unset, because `heifload` is flagged untrusted; same container sandbox). A video container (`ftyp` non-HEIF brands, EBML, RIFF AVI, MPEG-TS sync bytes) takes the poster path below. Anything else: `Unsupported`. SVG/PDF/Magick loaders are compiled out of the image (CI check), so an SVG named `.png` is `Unsupported`.
2. `vipsheader` (same profile) reads width/height; above `photo_max_pixels` (default 250 MP) -> `LimitExceeded{pixels}` before any decode (30000x30000 corpus case).
3. `vipsthumbnail in/source --size <edge> --path out/result.webp[Q=<from ParamSet>,keep=none]`: EXIF orientation applied (default auto-rotate; dimensions swap for orientations 5-8), metadata stripped, **never `--linear`**, never libheif "unlimited". The 320 px thumb of a HEIC may come from the embedded thumbnail; the 1440 px preview needs a full HEVC decode (AC-10 risk, measured in gate S4). Exact option strings are verified in S4.
4. Output must be a regular WebP of at most `max_output_bytes`; real width/height are read back from the result, not assumed.

### Video poster (`render_still` on a video container)
ffmpeg (same whitelists) decodes one frame at `poster_at_ms` (computed by 09 in `media-shared::policy` and passed in: 1 s, or 10% of the duration for clips shorter than 10 s; the decoder does not decide it; HDR frames tone-mapped as below) to a PNG in `work/`; that PNG, which the runner itself produced, then goes through image Profile A to the WebP.

### Video (ffmpeg 8.x/9.x with libx264, native `aac`, libzimg; GPL image)
- **Input options on every ffmpeg/ffprobe call**: `-nostdin -protocol_whitelist file -format_whitelist <demuxers> -codec_whitelist <decoders>`; initial lists from the fact card (`mov,matroska,avi,mpegts` and the phone codecs h264, hevc, vp9, av1, aac, opus, mp3), finalised by gate S5 and the red-team corpus. No `concat`, `hls`, `sdp`, `data`, `tee` (also compiled out). Reads `in/source` as a **file, not a pipe** (`moov` at the end gives zero frames from `pipe:0`).
- **Probe**: `ffprobe -v error -show_streams -show_format -of json` with the same whitelists, parsed into `Probe`; `moov_first` comes from a bounded scan of top-level MP4 box headers (at most 64 boxes, header bytes only). The decoder reports facts and never judges compliance.
- **Mapping**: first video and first audio stream only, no subtitles/data/chapters, `-map_metadata -1` (GPS and title tags are not copied).
- **Remux** (`Plan::Remux`): `-c copy -movflags +faststart` with the same mapping; no filters, no re-encode. Output is probed like a transcode result.
- **CPU encode**: `libx264 -preset veryfast -crf 23 -profile:v high -pix_fmt yuv420p -maxrate 10M -bufsize 20M`, `-c:a aac -b:a 128k -ac 2`, `-movflags +faststart`, BT.709 tags. `maxrate` keeps the *output* inside the compliance rule. Values come from `Params` via the plan version, not from env.
- **Scale**: orientation independent, short edge `min(1080, short)`, long edge capped at 1920, `-2` even dimensions; rotation metadata is applied by ffmpeg's default autorotate (do not pass `-noautorotate`; AC-45 orientation).
- **HDR (HLG/PQ) to SDR**: downscale first, then `zscale` linear light -> `tonemap=hable` -> `zscale` BT.709 -> `yuv420p`. Order and operator tuned in S5 against the 20 min budget for 10 min of 4K HLG.
- **VFR and audio sync**: `-fps_mode vfr` plus `aresample=async=1`; drift <= 40 ms checked by FT-P-27.
- **NVENC [v1.1]**: at start (and cached) one real test encode per listed encoder (`ffmpeg -f lavfi -i nullsrc=s=256x256:d=0.1 -c:v h264_nvenc -f null -`); `-encoders` alone proves nothing. Only `h264_nvenc` is used; VAAPI/QSV/VideoToolbox stay experimental and unused. Plan flags: `-c:v h264_nvenc -preset p5 -tune hq -rc vbr -cq 23 -b:v 0 -profile:v high` on **system-memory frames** because the image is built without CUDA/NPP filters (so no `scale_cuda`); GPU concurrency capped at the probed session limit (12 on GeForce). Non-zero exit -> `EncoderFailed{exit_code}`; the supervisor reruns on CPU.
- **Output validation**: after a remux or encode `ffprobe` the result; duration within 1 s of input, one H.264 video stream; the probe goes back so 09 can apply the compliance check.

### Self-check
Attempts a connect/resolve to each `forbidden_targets` entry (all must fail), verifies `environ` contains only the allowlist, `mountinfo` lacks the blob path and secrets, `uid != 0`, `CapEff == 0`, `no_new_privs`, root filesystem read-only. Reports data; the supervisor decides.

## 5. Data, side effects, dependencies
No persistent data. Writes only under `out/` and `work/` of the current job directory; creates the socket at start (mode 0660, group `decoder_gid` shared with the supervisor; the scratch tree uses the same group, so no world-readable mode is needed). Child processes and temp files die with the job. Depends only on `media-shared` and helpers 01, 02. **Implement after**: `media-shared` types and `decoder_protocol` (implemented first, owned here). **Parallel with** 09. **Blocks**: 09 integration tests.

## 6. Configuration (env, no secrets; keys and env names as in helper 01)
| Key | Default | Notes |
|---|---|---|
| `decoder_socket_path`, `decoder_gid` | `/run/decoder/decoder.sock`, set in Compose | one socket per container; `decoder_gid` is the group shared with the supervisor |
| `decoder_scratch_root` | `/scratch` | shared with the supervisor only; `decoder-interactive`: tmpfs, 512 MiB; `decoder-general`: dedicated disk mount, 24 GiB (09 section 6 gives the arithmetic) |
| `photo_cpu`, `photo_memory`, `photo_job_timeout` | 2, 2GiB, 120s | AC-25 ceilings (the supervisor sends the same values; requests are clamped) |
| `video_cpu`, `video_memory`, `video_job_timeout` | 4, 4GiB, 40m | |
| `decoder_stall_kill`, `decoder_alive_interval` | 90s, 5s | AC-45 |
| `decoder_watchdog` | 20s | self-abort when the main loop is silent |
| `decoder_memory_poll_interval` | 1s | |
| `photo_max_pixels` | 250000000 | |
| `decoder_max_output_photo` / `decoder_max_output_video` | 32MiB / 10GiB | video output is at most the input size limit (a remux is as large as its input); S5 may lower it |
| `decoder_format_whitelist`, `decoder_codec_whitelist` | see section 4 | |
| `nvenc` | off | `on` enables the test-encode probe [v1.1] |
The container adds cgroup `cpus`/`mem_limit` at the video ceiling (`decoder-interactive`: the photo ceiling), `pids_limit`, `read_only`, `cap_drop: ALL`, `no-new-privileges`, the seccomp profile above, `init: true`, `restart: unless-stopped`, `network_mode: none` (the socket volume shared with the supervisor is the only way in or out); per-kind limits below the ceiling are enforced by the process guard.

## 7. Security notes
Decoders are assumed compromised. They hold no token, no key, no DB or blob access. A compromised decoder can read everything inside its own container (the current job's input, its scratch, the socket) and can send arbitrary bytes on the socket and write arbitrary files under `out/`. What contains it: one job at a time, kill-all-descendants and a wiped job tree after every job, exit-and-restart after any hostile-class outcome, no `ptrace`/`process_vm_*`, no network, no credentials, and a supervisor that treats every frame and every output file as hostile (strict schema, safe open, `fstat`, re-validation by the server's sniffer). `Plan` is an enum from `media-shared`, never argv text from the supervisor. All child output is bounded and sanitised before it enters a log or a `last_error`. libheif limits stay at the compiled defaults (never unlimited); the container memory limit is the real bound. Weekly image rebuild and 0 HIGH/CRITICAL CVEs are CI/release gates (AC-63, NFT-SEC-36), not runtime code.

## 8. Logging
JSON, `component=decoder-runner`, `job_id`, `kind`, `profile`, kill reason, exit code, durations. No paths of user content, no stderr beyond a 2 KiB sanitised tail on kills. Metrics via the supervisor (`decoder_kills_total{reason}`, `worker_decoder_restarts_total`); the runner exposes only `decoder-runner health` (informational: restarts come from the self-abort, not from the healthcheck).

## 9. ACs implemented
| AC | Release | Part |
|---|---|---|
| AC-10 | v1.0 | WebP 320/1440, orientation swap 5-8, HEIC profile, SSIM >= 0.95 vs reference, video poster at the supplied offset |
| AC-11 | v1.0 | H.264 High 1080p AAC MP4 faststart, lossless remux, 4K HLG tone-map, speed budgets |
| AC-25 | v1.0 | whitelists, `VIPS_BLOCK_UNTRUSTED`, limits, hostile corpus -> failure, self-check, containment (subreaper, kill-all, seccomp, exit on hostile outcome) |
| AC-28 | v1.1 | NVENC probe by test encode, exit-code reporting |
| AC-44 | v1.0 | `maxrate`/CRF keep derived at most 20% of original |
| AC-45 | v1.0 | kill within 90 s of last progress; orientation and A/V drift |
**Interaction ACs folded in**: **AC-71** [v1.0]: the runner never keeps state between jobs, removes `work/` on every exit path, and treats socket loss as cancel, so a killed supervisor leaves neither CPU burn nor scratch residue that survives the supervisor's start-up sweep. **AC-72** [v1.0]: the runner trusts only `in/source` that 09 verified by hash.

## 10. Tests
FT-P-22 to FT-P-28, FT-N-06, FT-N-07, FT-N-08, NFT-PERF-15, NFT-PERF-16, NFT-PERF-18, NFT-PERF-19, NFT-PERF-21, NFT-RES-19, NFT-RES-31, NFT-RES-LIM-04, NFT-RES-LIM-09, NFT-SEC-28, NFT-SEC-29, FT-P-74 (build properties). Unit: sniffing table, argv builders (golden, including remux), probe parsing for photos and videos, box scanner, guard kills (wall, memory, stall using a test child). Containment tests against the real image: a child calls `setsid` (and double-forks) and tries to read the next job's `in/source` after the job ended: it must be dead before the next request is accepted; `ptrace` and `process_vm_readv` return EPERM; a hostile-class outcome makes the runner exit and the container restart; a frozen main loop makes the runner abort within `decoder_watchdog`; a FIFO planted as `in/source` is rejected. Red-team corpus run against the real image: SVG/PDF renamed `.png`, HLS playlist to `/etc/passwd`, concat file, 30000x30000 PNG, 20 GB sparse video, truncated HEIC, zip bomb inside metadata. Remux: index-at-end clip yields the same streams (hash of the packets) with the index first.

## 11. Component acceptance
Every corpus item ends in a typed failure with no data exposure and the next job still runs; limits hold (photos 2 CPU/2 GiB/120 s, video 4 CPU/4 GiB/40 min); a stalled ffmpeg is gone within 90 s of its last progress line; orientation 5-8 and rotated/VFR clips come out right; a remux copies streams without re-encoding; with no GPU or a failing GPU the CPU path is used and the output is valid.

## 12. Risks and open points
0. Defaults chosen at Step 5 gap pass: a descendant found and killed at job end is hostile-class (the runner exits); an SVG renamed `.png` is `Unsupported` (not `Corrupt`); `DecodeError` gains `ProtocolViolation`.
0. Container isolation shares the host kernel; the containment above reduces cross-job leakage but is not a VM boundary. A kernel escape is out of scope for v1.0.
1. HEIC 1440 px full decode may miss the 5 s p95 (unmeasured); fallback is deriving the 320 px from the preview, decided in S4.
2. RSS polling at 1 s can miss a fast allocation spike; the container cgroup limit is the hard stop and the failure is then an OOM-killed container (supervisor sees socket loss -> transient, `decoder_kills_total{memory}`).
3. NVENC without CUDA filters means CPU decode/filter plus GPU encode only; the 1.3x speed-up (AC-28) may not hold for tone-mapped HDR (gate S10).
4. Whitelist contents must match phone corpora; too tight loses legit clips, too loose weakens AC-25 (gate S5).
5. `ffmpeg -progress` to `pipe:1` while `-protocol_whitelist file` is set: verify the progress sink is not subject to the input whitelist (S5).
6. `probe` on a photo is new relative to the video-only probe: it must stay a header read (no decode) so it adds no measurable latency to AC-10.
