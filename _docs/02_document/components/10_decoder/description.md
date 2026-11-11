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
- **Self-abort watchdog**: Docker does not restart a container whose healthcheck fails, so the runner polices itself. The main loop bumps a monotonic counter at least every `alive_interval`; a watchdog thread calls `abort()` when the counter has not moved for `decoder_watchdog` (default 20 s, i.e. four intervals) whether a job is running or idle. The container exits and is restarted. **Test-only trigger (decided in AZ-3851, G-10-4)**: cargo feature `test-freeze` of the `decoder-runner` crate itself (no other crate depends on the runner, so feature unification cannot switch it on), set only by the `test-stubs` image build. With it the runner installs a `SIGUSR1` handler that sets an atomic flag; the one place that bumps the watchdog counter checks the flag and then blocks the main loop for good, while the watchdog thread (it masks `SIGUSR1` with `pthread_sigmask`, so the handler runs on the main thread) keeps running and calls `abort()`. The harness sends the signal with `docker kill --signal=USR1` (the `init: true` process forwards it to the runner), idle or mid-job; `SIGSTOP` cannot do this because it stops the watchdog too. Flag check, handler and the marker string `test_freeze_main_loop` all sit under `#[cfg(feature = "test-freeze")]`, so the release build has no changed code path. Proof for the release image (CI check, same family as the `test-clock` rule): `strings` and `nm` of `decoder-runner` in the pushed release image show no `test_freeze_main_loop` and no `test_freeze` symbol, and `cargo tree -p decoder-runner -e features` lists no `test-freeze`. E-19/c07b implements it, E-19/c15 builds the test image, IT-10-18 uses it. Options compared and PoC measurements: section 13.6.
- One job at a time and a wiped tree per job mean a compromised child can neither outlive its job nor read the next job's input (the next `in/source` is created after the kill-all and only then does the runner accept another request). This is a hardening of a shared-kernel container, not a VM boundary; no gVisor in v1.0.

## 4. Execution profiles

### Image (libvips 8.18.x, CLI only, never the crate)
1. Sniff magic bytes in the runner: JPEG, PNG, WebP, GIF -> **Profile A** (`VIPS_BLOCK_UNTRUSTED=1`); HEIC/HEIF/AVIF (`ftyp` brands) -> **Profile B** (variable unset, on the premise that `heifload` is flagged untrusted; same container sandbox). Gate S4 found that in libvips 8.18.0 `heifload` is NOT flagged untrusted (a HEIC loads with `VIPS_BLOCK_UNTRUSTED=1`), so Profile B may collapse into Profile A: the image CI check `VIPS_BLOCK_UNTRUSTED=1 vipsthumbnail <heic>` on the shipped 8.18.7 decides (section 13.7); until then the rule above stands. A video container (`ftyp` non-HEIF brands, EBML, RIFF AVI, MPEG-TS sync bytes) takes the poster path below. Anything else: `Unsupported`. SVG/PDF/Magick loaders are compiled out of the image (CI check), so an SVG named `.png` is `Unsupported`.
2. `vipsheader` (same profile) reads width/height; above `photo_max_pixels` (default 250 MP) -> `LimitExceeded{pixels}` before any decode (30000x30000 corpus case).
3. Exact argv (verified in gate S4 on libvips 8.18.0, section 13.7; E-19/c08 builds it): `vipsthumbnail <in/source> --size <edge>x<edge>> --output-profile srgb --path <absolute job_dir>/out/result.webp[Q=<from ParamSet>,keep=none]`, env `VIPS_CONCURRENCY=<cpu>` and, for Profile A, `VIPS_BLOCK_UNTRUSTED=1`. The trailing `>` of `--size` means shrink only (without it a 400x300 image is enlarged to 1440x1080); EXIF orientation is applied by default (dimensions swap for 5-8; never `--no-rotate`); `keep=none` is mandatory (without it EXIF, GPS and the maker string survive in the WebP); `--output-profile srgb` is mandatory (without it a Display P3 photo, the iPhone default, keeps its P3 numbers and comes out desaturated); never `--linear`, never libheif "unlimited" (`heifload` has an `unlimited` option that is simply not passed). `--path` takes the full output path; `-o` is relative to the input's directory and must not be used. The 320 px thumb of a HEIC is a full decode of the HEIC, not the embedded thumbnail and not derived from the preview (decision and costs: section 13.7).
4. Output must be a regular WebP of at most `max_output_bytes`; real width/height are read back from the result, not assumed.

### Video poster (`render_still` on a video container)
ffmpeg (same whitelists) decodes one frame at `poster_at_ms` (computed by 09 in `media-shared::policy` and passed in: 1 s, or 10% of the duration for clips shorter than 10 s; the decoder does not decide it; HDR frames tone-mapped as below) to a PNG in `work/`; that PNG, which the runner itself produced, then goes through image Profile A to the WebP.

### Video (ffmpeg 8.x/9.x with libx264, native `aac`, libzimg; GPL image)
- **Input options on every ffmpeg/ffprobe call**: `-nostdin -protocol_whitelist file -format_whitelist <demuxers> -codec_whitelist <decoders>`; final lists from gate S5 (section 13.1): `-format_whitelist mov,matroska,avi,mpegts` and `-codec_whitelist h264,hevc,vp9,libdav1d,aac,opus,mp3float,mov_text,webvtt`. The codec list names **decoders, not codecs**: `av1` and `mp3` do not match the `libdav1d` and `mp3float` decoders ffmpeg picks, and an unlisted decoder fails `ffprobe`; the AV1 entry must be the decoder the image build enables. `ffprobe` takes the same three options but not `-nostdin` (it rejects it). No `concat`, `hls`, `sdp`, `data`, `tee` (also compiled out). Reads `in/source` as a **file, not a pipe** (`moov` at the end gives zero frames from `pipe:0`).
- **Probe**: `ffprobe -v error -show_streams -show_format -of json` with the same whitelists, parsed into `Probe`; `moov_first` comes from a bounded scan of top-level MP4 box headers (at most 64 boxes, header bytes only). The decoder reports facts and never judges compliance.
- **Mapping**: first video and first audio stream only, no subtitles/data/chapters, `-map_metadata -1` (GPS and title tags are not copied).
- **Remux** (`Plan::Remux`): `-c copy -movflags +faststart` with the same mapping; no filters, no re-encode. Output is probed like a transcode result.
- **CPU encode**: `libx264 -preset veryfast -crf 23 -profile:v high -pix_fmt yuv420p -maxrate 10M -bufsize 20M`, `-c:a aac -b:a 128k -ac 2`, `-movflags +faststart`, BT.709 tags. `maxrate` keeps the *output* inside the compliance rule. Values come from `Params` via the plan version, not from env.
- **Scale**: orientation independent, short edge `min(1080, short)`, long edge capped at 1920, `-2` even dimensions; rotation metadata is applied by ffmpeg's default autorotate (do not pass `-noautorotate`; AC-45 orientation).
- **HDR (HLG/PQ) to SDR** (fixed by gate S5, section 13.2): `-vf scale=1920:1080:flags=bicubic,zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,format=yuv420p` (the width and height come from the plan's scale step; the downscale must stay first: tone mapping at 4K first costs 2.3 to 3 times more). Budget check on REF-CPU4 is NOT-RUN (section 13.2).
- **Progress sink**: `-nostats -loglevel error -progress pipe:1` (stdout is the runner's pipe; the output is a file). It is not subject to `-protocol_whitelist file` (section 13.2). A block is the lines up to `progress=continue|end`; the stall timer resets on every block, not on a value, because `out_time_us` and `speed` read `N/A` at the start. `out_time_ms` in ffmpeg 8 is microseconds (same as `out_time_us`); the runner reports `Progress.out_time_ms` as `out_time_us / 1000`.
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
| `decoder_format_whitelist`, `decoder_codec_whitelist` | `mov,matroska,avi,mpegts`; `h264,hevc,vp9,libdav1d,aac,opus,mp3float,mov_text,webvtt` | gate S5 (section 13.1); decoder names, not codec names |
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
FT-P-22 to FT-P-28, FT-N-06, FT-N-07, FT-N-08, NFT-PERF-15, NFT-PERF-16, NFT-PERF-18, NFT-PERF-19, NFT-PERF-21, NFT-RES-19, NFT-RES-31, NFT-RES-LIM-04, NFT-RES-LIM-09, NFT-SEC-28, NFT-SEC-29, FT-P-74 (build properties). Unit: sniffing table, argv builders (golden, including remux), probe parsing for photos and videos, box scanner, guard kills (wall, memory, stall using a test child). Hostile-behaviour tests use the `test-stubs` image variant (section 13.4). Containment tests against the real image: a child calls `setsid` (and double-forks) and tries to read the next job's `in/source` after the job ended: it must be dead before the next request is accepted; `ptrace` and `process_vm_readv` return EPERM; a hostile-class outcome makes the runner exit and the container restart; a frozen main loop makes the runner abort within `decoder_watchdog`; a FIFO planted as `in/source` is rejected. Red-team corpus run against the real image: SVG/PDF renamed `.png`, HLS playlist to `/etc/passwd`, concat file, 30000x30000 PNG, 20 GB sparse video, truncated HEIC, zip bomb inside metadata. Remux: index-at-end clip yields the same streams (hash of the packets) with the index first.

## 11. Component acceptance
Every corpus item ends in a typed failure with no data exposure and the next job still runs; limits hold (photos 2 CPU/2 GiB/120 s, video 4 CPU/4 GiB/40 min); a stalled ffmpeg is gone within 90 s of its last progress line; orientation 5-8 and rotated/VFR clips come out right; a remux copies streams without re-encoding; with no GPU or a failing GPU the CPU path is used and the output is valid.

## 12. Risks and open points
0. Defaults chosen at Step 5 gap pass: a descendant found and killed at job end is hostile-class (the runner exits); an SVG renamed `.png` is `Unsupported` (not `Corrupt`); `DecodeError` gains `ProtocolViolation`.
0. Container isolation shares the host kernel; the containment above reduces cross-job leakage but is not a VM boundary. A kernel escape is out of scope for v1.0.
1. HEIC 1440 px full decode and 5 s p95: gate S4 measured the pair at p95 1.05 s on a developer workstation (INFORMATIONAL, synthetic 12 MP tiled HEIC, 2 CPUs; target 1.5 s, failure 3 s); decision: full decode for both sizes, deriving the 320 px from the preview is the recorded fallback (section 13.7). Gate numbers on REF-CPU4 and on real iPhone HEIC files are NOT-RUN.
2. RSS polling at 1 s can miss a fast allocation spike; the container cgroup limit is the hard stop and the failure is then an OOM-killed container (supervisor sees socket loss -> transient, `decoder_kills_total{memory}`).
3. NVENC without CUDA filters means CPU decode/filter plus GPU encode only; the 1.3x speed-up (AC-28) may not hold for tone-mapped HDR (gate S10).
4. CLOSED for the lists (gate S5, section 13.1, measured on SYNTHETIC clips; no real phone clips were available): lists fixed, every synthetic phone-style clip passes, every DS-16 container trick is refused. Open residue: real iPhone/Android clips can carry streams or codecs no synthetic clip had (ProRes, APAC spatial audio, AMR audio, Dolby Vision metadata) and would fail `Unsupported`; check with real clips at gate S5.
5. CLOSED (gate S5, section 13.2): `-progress pipe:1`, `pipe:2`, `file:` and `unix:` all deliver blocks every 0.5 s under `-protocol_whitelist file` (it is an input option and does not touch the progress sink); `pipe:1` is chosen.
6. `probe` on a photo is new relative to the video-only probe: it must stay a header read (no decode) so it adds no measurable latency to AC-10.

## 13. Spike records (gates S4 and S5, AZ-3849, AZ-3850, AZ-3851)

Measured 2026-11-11 on a developer workstation (AMD Ryzen 9 3900XT, ffmpeg 8.0.1, libvips 8.18.0, libheif 1.21.2; the image ships libheif >= 1.22.0, libvips 8.18.7). **It is not REF-CPU4. Every media file is SYNTHETIC (ffmpeg lavfi, libvips, heif-enc); no real phone clip, no real iPhone HEIC and no real HLG phone clip was available.** Only functional results are results; every time below is INFORMATIONAL and the gate numbers are `NOT-RUN(HW-REF)` (leftover `_docs/_process_leftovers/2026-11-11_az-3849-3850-3851-decoder-spikes.md`).

### 13.1 Whitelists (AC-25)
Final lists: `-protocol_whitelist file -format_whitelist mov,matroska,avi,mpegts -codec_whitelist h264,hevc,vp9,libdav1d,aac,opus,mp3float,mov_text,webvtt`. Findings that changed the initial list: `-codec_whitelist` matches decoder names, so `av1` (decoder in use `libdav1d`) and `mp3` (`mp3float`) refused clips that were meant to pass; a subtitle track makes `ffprobe` fail (exit 1, truncated JSON) unless its decoder is listed (`mov_text` in MP4, `webvtt` in WebM; `subrip` and `ass` in MKV needed no entry); data tracks (`tmcd`) need nothing; `ffprobe` rejects `-nostdin`. Not listed on purpose: `mpeg4` video (old camera AVI), `prores`, `amr_nb`: such a clip fails `Unsupported`. The format entries `avi` and `mpegts` are kept from the fact card although no phone writes them (desktop folders do); dropping them is an owner call with no evidence either way.

| Clip (SYNTHETIC) | Format / streams | Result with the final lists |
|---|---|---|
| p01 DS-07-like | mp4, h264 High + aac, faststart | probe and transcode pass |
| p02 | mp4, hevc 8-bit + aac | pass |
| p03 DS-35-like | mp4, h264 + aac, moov at end | pass |
| p04 | webm, vp9 + opus | pass |
| p05, p16 | mkv and mp4, av1 + opus | pass (needs `libdav1d`) |
| p06 | mp4, h264 + mp3 | pass (needs `mp3float`) |
| p07 | avi, mpeg4 + mp3 | refused (`mpeg4` not listed), by design |
| p08 | mpegts, h264 + aac | pass |
| p09 | mov, h264 + aac + timecode data track | pass |
| p12, p13 DS-08-like | mp4 rotated 90 (display matrix); VFR | pass |
| p14 | mp4, h264 + aac + `mov_text` subtitle, title and GPS-style tags | pass (needs `mov_text`); transcode drops tags with `-map_metadata -1` |
| p15 | mov, hevc Main10 HLG 1080p + aac | pass |
| p17 | webm, vp9 + opus + webvtt | pass (needs `webvtt`) |
| p18 | m4a, aac only | pass |
| DS-16 `.m3u8` text named `.mp4` (to `file:///etc/passwd`, `/proc/self/environ`, a `data:` URI) | text | refused, no bytes of the target in any output or log |
| DS-16 concat file (also with `subfile`), SDP | text | refused: `Format not on whitelist` |
| same files with `.m3u8`, `.ffconcat`, `.sdp` names | text | refused: `Format not on whitelist 'mov,matroska,avi,mpegts'` |
| DS-16 20 GB sparse video (valid ftyp, moov, 20 GiB `free` box) | mp4 | probe and transcode succeed: a structurally valid MP4 is not refused by any list; the limits contain it (IT-10-20). The AC-1 sentence "every DS-16 video item is refused" is wrong for this item |

Layers: with no whitelist at all, ffmpeg 8.0.1 already refuses the DS-16 items as named (HLS needs a `.m3u8` name, concat checks "Unsafe file name"); with only `-protocol_whitelist file` the `.m3u8` variant stops at the segment-extension check and `http:` and `rtp` at the protocol whitelist; the format whitelist stops every one of them before parsing. The decoder reads `in/source`, which never has an extension. Not covered (no sample): external `dref` data references in MOV, which ffmpeg disables by default.

### 13.2 Progress sink and HDR chain (AC-45, AC-11)
**Progress.** Under `-protocol_whitelist file` the sinks `-progress pipe:1`, `pipe:2`, `file:<path>` and `unix:<socket>` all work (runs of 36 to 135 s, 72 to 270 blocks, a block every 0.5 s, longest gap 0.72 s while the host was loaded); `fd:N` is rejected. The whitelist is an input option and does not apply to the sink. Chosen: `pipe:1` with `-nostats -loglevel error`. Parsing rules in section 4.

**HDR chain, 4 pinned cores (distinct physical cores, SMT siblings idle), `-threads 4 -filter_threads 4`, libx264 `-preset veryfast -crf 23 -maxrate 10M`, 30 s synthetic 4K HLG HEVC Main10 (40 Mbit/s): INFORMATIONAL, host also loaded by other work.**

| Chain | Wall s per s of media (two runs) | Decision |
|---|---|---|
| A: `scale` to 1080p, `zscale` linear, `tonemap=hable`, `zscale` BT.709, `yuv420p` | 1.38, 1.40 | chosen |
| C: resize inside the first `zscale` | 1.36, 1.45 | same speed, SSIM 0.999 against A; no reason to change |
| B: tone map at 4K, then downscale | 3.58, 4.25 | rejected (2.3 to 3 times slower) |

The gate figure (budget 2.0 s/s on REF-CPU4, AC-3 of the spike) is NOT-RUN; the chain is provisional and no workstation number is a gate result. Output check: 1920x1080 `yuv420p`, High, BT.709 tags. **Luma tolerance proposed for IT-10-10 (SYNTHETIC, confirm on the real DS-06 base):** mean luma in 8-bit code values within 2.0 of the harness render of the same chain on the same file (full clip: encode noise at most 0.2, chain B versus A 0.06, C versus A 0.02); a missing tone map is 60 code values off and is caught. The SDR source the generator started from is not a usable reference: tone mapping moves mean luma from 201 to 120 by design.

### 13.3 HLG poster-frame SSIM, input to the owner (G-10-6)
Method: SYNTHETIC 4K HLG clip (SDR BT.709 master converted with `zscale` to BT.2020 `arib-std-b67`, HEVC Main10), poster frames at 1 s and at 10% of 30 s (3 s) through chain A to a 1080p PNG, then `vipsthumbnail --size 320 --path ...[Q=80,keep=none]` to WebP; reference frames from the SDR master scaled to the same size and thumbnailed the same way; SSIM by ffmpeg `ssim` (Y, and All = mean of the three planes). Owner decision 2026-11-11: the reference for the HLG poster-frame SSIM is the harness render of an equivalent chain (measured 0.995 or more), not the SDR master the generator started from (0.85 to 0.91). With that reference the AC-10 tolerance of 0.95 is comfortably met on the synthetic clip; the tolerance value itself is still the owner's to confirm (recommended: the same 0.95 as AC-10), so IT-10-7 (HLG part) keeps recording only until then.

| Frame | Compared with | 1080p PNG Y / All | 320 px WebP Y / All |
|---|---|---|---|
| 1 s | SDR master (generator reference) | 0.841 / 0.888 | 0.847 / 0.896 |
| 3 s | SDR master | 0.856 / 0.898 | 0.862 / 0.906 |
| 1 s | chain B poster (harness uses another order) | 0.993 / 0.995 | 0.996 / 0.997 |
| 3 s | chain B poster | 0.993 / 0.994 | 0.995 / 0.997 |
| 1 s | chain C poster | 0.999 / 0.999 | 0.996 / 0.997 |
| 3 s | chain C poster | 0.999 / 0.999 | 0.996 / 0.997 |

What the owner needs to know: the number depends on what "reference" means. Against the SDR master the tone-mapped frame scores 0.85 to 0.91 because tone mapping changes luma on purpose, so the image tolerance of 0.95 cannot be used; against a harness render of an equivalent chain it scores 0.995 or more. The tolerance, and the definition of the reference, stay the owner's decision.

### 13.4 Test stubs mechanism (G-10-1, E-19/c15 builds it)
- Build: Dockerfile target `test-stubs` (`FROM runtime`) built with `FEATURES=test-freeze`, tag `<tag>-test`, never promoted. The release image is the `runtime` target and contains none of this.
- One compiled stub binary (a small Rust program, not a script: ST-10-2 requires that no `/bin/sh` is ever started) replaces `ffmpeg`, `ffprobe`, `vipsthumbnail` and `vipsheader`; the real tools are moved to `<name>.real` in the same directory. The runner has no config key or env var that selects binaries, so nothing exists to misconfigure in release.
- Marker: the runner sniffs magic bytes first, so a marker file must look like a valid image or container. The marker is the text `SCSTUB1 <behaviour> [args]` inside a JPEG COM segment (image jobs) or an MP4 `free` box after `ftyp` (video jobs); the stub scans the first 4 KiB of its input (the path in argv, which may be `/proc/self/fd/N`). Behaviours: `hang`, `alloc <MiB>`, `ignore-term`, `setsid-fork`, `write-next-job`, `ptrace`, `stderr <KiB>`, `write <MiB>`, `spin`; `vipsheader` and `ffprobe` answer fixed facts from the marker. No marker: append argv and environment as one JSON line to `<scratch_root>/_stub/<job id>.jsonl` and `exec` the real tool (this is the "argv log" of IT-10-5, IT-10-9, IT-10-12, ST-10-2).
- Proof the release image has no stub (CI, on the pushed digest): `docker export | tar -t` has no `decoder-test-stub` and no `*.real`; `sha256sum` of the four tool paths equals the build-stage manifest; `strings decoder-runner` has no `SCSTUB1` and no `test_freeze_main_loop`; the image has no label `org.sunnycloud.variant=test-stubs`, and the harness refuses a perf run on an image that has it.

### 13.5 Manifest requirement for the 30 s base files (G-10-3, on E-31/c21)
`gen_video.py` must keep the base file before looping and write both into the manifest of DS-05 and DS-06: `clip-1080-sdr.base30.mp4` (duration 30.000 s, 1080p 30 fps H.264 at 18 to 20 Mbit/s, about 70 MB) and `clip-4k-hlg.base30.mp4` (30.000 s, 3840x2160 30 fps HEVC Main10, BT.2020 `arib-std-b67` tags, about 40 Mbit/s, about 150 MB); per file: `path`, `duration_ms`, `size_bytes`, `sha256`, stream facts, and for the 10-minute loop file `base_file` and `loops` (20) so a test can find its base. IT-10-7, IT-10-8, IT-10-10, IT-10-13 and AT-10-2 read the base entries; the loops stay for NFT-PERF-15/16. G-10-3 stays open until E-31/c21 ships them.

### 13.6 Watchdog trigger options (G-10-4)
| Option | Blocks only main loop | Release code path unchanged | Provably absent from release | Verdict |
|---|---|---|---|---|
| Feature-gated protocol request | yes | no: a new request variant in `media-shared`, which 09 links too (feature unification can switch it on there) and which the hostile-input parser must parse | weak | rejected |
| Feature-gated stub marker in `in/source` | yes, but only while a job runs | no: test code in the job path | yes | rejected (cannot freeze an idle runner) |
| Feature-gated `SIGUSR1` handler (chosen) | yes, idle or mid-job | yes: compiled out | yes: `strings` and `nm` | chosen |

PoC of the mechanism (throwaway program with a 3 s watchdog, not the runner): signal while idle and while polling a child, watchdog `abort()` after 3.0 s of silence in both cases (exit 134; about 4.3 s from the signal including the host core-dump handler, so the runner sets its own `RLIMIT_CORE=0`); `SIGSTOP` never aborted it; the build without the feature has 0 marker strings and 0 symbols, the build with it 1 and 2.

### 13.7 Image option strings and the HEIC decisions (gate S4)
- Orientation: stored image built from a four-corner picture for each EXIF orientation 1 to 8 (by the EXIF definition, not by libvips): every output is upright, 200x150 from 300x400 stored for 5 to 8. A 4000x3000 JPEG tagged 5, 6 and 8 gives 1080x1440 at 1440 (tag 1 gives 1440x1080). A HEIC with an `irot` transform (angle 270) comes out upright too; `heifload` applies `irot`.
- Metadata: with `keep=none` the WebP has a single `VP8 ` chunk (no EXIF, ICCP, XMP; no GPS or maker string in the bytes) for a JPEG with orientation, GPS and maker tags and for a tiled HEIC with GPS tags; without it the EXIF chunk and the maker string stay.
- Colour: a Display P3 JPEG converted to sRGB with `--output-profile srgb` differs from the sRGB original by 0.87 mean (code values); without the option by 7.0 (the P3 numbers kept, no profile written). sRGB sources are unchanged by the option.
- Size: `--size 1440x1440>` leaves a 400x300 image at 400x300; plain `--size 1440` enlarges it.
- Untrusted block: under `VIPS_BLOCK_UNTRUSTED=1` JPEG works, an SVG named `.png` is `not a known file format` (it loads when the variable is unset on this host, whose libvips has the SVG loader), and a HEIC still loads on libvips 8.18.0 (see section 4 step 1). The libheif security limits in the shipped image could not be checked here (image not built); CI check suggested: a HEIC above the compiled pixel limit fails `Unsupported` or `LimitExceeded`.
- Timing, 1 slot, 2 CPUs (`taskset`, `VIPS_CONCURRENCY=2`), warm cache, one process per size, SYNTHETIC 12 MP files (JPEG Q85 noise textures; HEIC 4000x3000 tiled 512, q 50, with an embedded 320 px thumbnail): INFORMATIONAL, targets PT-10-1.

| Set | Option | Pair p50 / p95 | Peak RSS |
|---|---|---|---|
| 100 JPEG | 1440 then 320, each from the file | 0.37 s / 0.43 s (target 0.6 s) | 71 MB |
| 20 HEIC | full decode twice (chosen) | 0.99 s / 1.05 s (target 1.5 s) | 147 MB |
| 20 HEIC | 320 derived from the 1440 WebP | 0.64 s / 0.68 s | 146 MB |
| 20 HEIC | 320 from the embedded thumbnail (`heifload --thumbnail`) | 0.71 s / 0.73 s | 147 MB |

- Decision for the 320 px HEIC thumb: **full decode of the HEIC, same call as the preview.** It is within the 1.5 s target on the workstation with margin; the embedded thumbnail saves 0.28 s but is rejected because `vipsthumbnail` does not use it, its crop and orientation are chosen by the producing app (in the synthetic file it was portrait 240x320 for a landscape 4000x3000 main image) and it differed from the full-decode render (SSIM 0.76 on noisy synthetic content); deriving from the preview saves 0.36 s but the protocol gives the second `render_still` only `in/source`, so it would need a protocol change or runner state (against AC-71). Fallback if gate S4 on REF-CPU4 shows a HEIC pair p95 above 3 s: produce both sizes in one job from one decode (a protocol change owned by E-19/c02 and 09). REF-CPU4 timings and real iPhone HEIC files (tiled, `irot`, Display P3, possibly HDR gain map) are NOT-RUN.
