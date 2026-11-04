# Test Data Management

Source of truth for sets: `_docs/00_problem/input_data/data_parameters.md`. Expected results: `_docs/00_problem/input_data/expected_results/results_report.md` (cited as `RR <area> #n`, areas: Transfer, Sync, Deletion, Desktop, Media, Sharing, Rest, Auth, Web, Vault). Environment classes (`C`, `C-REF`, `H-WIN`, ...) are defined in `environment.md`.

## Generation rules (apply to every set)

- Nothing generated is committed. Generators live in `tests/data/gen/` (Python 3.13, `ffmpeg`, `ImageMagick`, `jpegtran`, `heif-enc`, `exiftool`, `blake3`), are run by `make test-data SEED=20261103 SET=<id|all>`, and write to `tests/data-cache/` (gitignored), mounted read-only as `/data` in the consumer, desktop containers and browsers; lab VMs get it through a shared directory or an image snapshot.
- Every generator is seeded and deterministic: same seed and generator version give byte-identical output. Randomness comes from one ChaCha20-based stream per file keyed by `BLAKE3(seed || set || file index)`, never from the system RNG.
- Each set writes `manifest.json`: path, size, BLAKE3 (file and per 2 MiB block), plus per-set expectations (for example orientation, expected derived count). **Expected hashes come from the generator, never from reading the SUT back.**
- Cache key = generator version + seed + parameters. A changed generator invalidates the cache. Free-licensed source media is fetched once from the URLs and SHA-256 values pinned in `tests/data/sources.lock`; if offline, the generator falls back to synthesized content and marks the manifest `fidelity: synthetic`.
- Disk budget on the generating host (REF-SRV): about 140 GB for all sets in the cache, plus `blobs-large` 200 GiB for the SUT. A hosted CI runner generates only the small sets (marked `S` below).

## Seed Data Sets

Sizes are for the default parameters. Columns: ID, set, generator, size, location, consumers (ACs; results rows), loading, cleanup.

| ID | Set | Generator (seeded) | Size | Cache path | Used by (AC; RR rows) | How loaded | Cleanup |
|----|-----|--------------------|------|-----------|-----------------------|-----------|---------|
| DS-01 S | many-small | `gen_tree.py`: 10,000 files, sizes log-uniform 1 KiB to 1 MiB, depth <= 5, names from a seeded word list, content from the file's stream | about 1.5 GB | `data-cache/many-small/` | AC-40; Transfer #7 | Consumer uploads via protocol client (single-request <= 2 MiB, batch commit); desktop tests copy into a sync root | Account purge via ctl; delete local copy |
| DS-02 S | listing-10k | `gen_listing.py`: one folder, 10,000 entries (9,500 files of 1 KiB, 500 subfolders, mixed-case and Unicode names) | about 10 MiB | `data-cache/listing-10k/` | AC-6; Sync #2 | Batch commit into one folder of a dedicated account, once per stack baseline (then folded into the golden snapshot `golden+listing`) | Reset to golden |
| DS-03 S | photos | `gen_photos.py`: 100 JPEG 4000x3000 (q85, 2.5-5 MB, from public-domain photographic bases plus seeded crops; falls back to synthetic textures), 20 HEIC 12 MP via `heif-enc`, EXIF Orientation 3/6/8 on 10 of the JPEGs (interpretation: the 10 are part of the 100; confirm) | about 450 MB | `data-cache/photos/` | AC-10, 9, 46, 53; Media #1, #8 | API upload; adb push to device or emulator `DCIM/Camera` | Account purge; `adb shell rm` |
| DS-04 | photos-bulk | `gen_photos_bulk.py`: 50 base 12 MP JPEGs, 200 lossless variants each with `jpegtran -copy all` plus a unique COM marker and a distinct EXIF DateTimeOriginal -> 10,000 unique files with real 12 MP decode cost | about 30 GB | `data-cache/photos-bulk/` | AC-10 (bulk); Media #2 | `perf-client` uploads all 10,000 on REF-LOAD at full speed while the interactive probe uploads one photo every 2 s | Account purge; reset |
| DS-05 | clip-1080-sdr | `gen_video.py clip-1080-sdr`: 30 s base from an open-licence source (Blender open movie, CC-BY), `-stream_loop` copy to 10:00, 1080p 30 fps SDR H.264 at about 18-20 Mbit/s video, i.e. above the 10 Mbit/s compliance limit, so it must be transcoded | about 1.2 GB | `data-cache/clips/` | AC-11 ref A; Media #3 | API upload; worker transcodes | Account purge |
| DS-06 | clip-4k-hlg | `gen_video.py clip-4k-hlg`: 4K SDR source converted to HLG with `zscale` (BT.2020, `arib-std-b67`), HEVC Main10 at about 40 Mbps with x265 `ultrafast` for a 30 s base, looped by stream copy to 10:00; colour tags set explicitly | about 3 GB | `data-cache/clips/` | AC-11 ref B; Media #4 | API upload | Account purge |
| DS-07 S | clip-compliant | `gen_video.py clip-compliant`: 30 s base (same open-licence source), looped by stream copy to 3:00; encoded once with `ffmpeg -c:v libx264 -profile:v high -level 4.1 -pix_fmt yuv420p -s 1920x1080 -b:v 6M -maxrate 8M -bufsize 12M -colorspace bt709 -color_primaries bt709 -color_trc bt709 -c:a aac -b:a 128k -movflags +faststart` (native AAC). `verify_compliant.py` then checks the confirmed definition and the generator fails if any check fails: profile Baseline/Main/High, long side <= 1920 and short side <= 1080, video bitrate <= 10 Mbit/s (average and worst 1-second window), AAC audio, `moov` before `mdat`, colour tags BT.709 SDR | about 150 MB | `data-cache/clips/` | AC-11, 44; Media #5 | API upload | Account purge |
| DS-08 | clip-mixed-100 | `gen_video.py mixed --mix mix.yaml`: 100 clips, 10 s to 2 min, composition from `mix.yaml` (H.264/HEVC, 720p to 4K, 15 rotated by display matrix 90/180/270, 15 variable frame rate via seeded `setpts` jitter, 10 audio-only, 20 already compliant by the DS-07 recipe); video carries flash frames and audio carries beeps at known timestamps so orientation and audio drift can be measured | about 4 GB | `data-cache/clips-mixed/` | AC-44, 45; Media #6, #7 | API upload; ctl SIGSTOP of ffmpeg for the stall case | Account purge |
| DS-09 | big-file-5g | `gen_bigfile.py --size 5GiB --dense`: incompressible stream; a 64-byte plaintext marker `SUNNYCLOUD-MARKER-<seed>` written every 1 MiB for the at-rest check | 5 GiB | `data-cache/big/big-5g.bin` | AC-2, 3, 7, 43; Transfer #1-3, Encryption #1 | `perf-client` / consumer upload; download hashed on the fly into `/dev/null` | Account purge; blob volume reset |
| DS-10 | big-file-50g | `gen_bigfile.py --size 50GiB --sparse-hybrid`: sparse file with a random 2 MiB block at every 64th block (about 800 MB allocated), whole-file and per-block BLAKE3 from a streaming pass | 50 GiB apparent, about 0.8 GB on disk | `data-cache/big/big-50g.sparse` | AC-4, 50, 51; Transfer #4, Web #1, #2 | Upload through API client (4 lanes) and through browsers (path-based file input) | Delete the 50 GiB server copy by account purge; browser downloads written to a scratch volume of >= 60 GiB, deleted after each run |
| DS-11 | tree-100k | `gen_tree.py --count 100000 --size 1KiB..16KiB --depth 6` | about 0.5 GB | `data-cache/tree-100k/` | AC-15; Desktop #1 | Linux container: bind mount. Windows: prebuilt inside a WIN-VM snapshot (creating 100,000 files on NTFS with Defender is slow) | Revert VM snapshot; remove directory |
| DS-12 S | tricky-names | `gen_names.py --os linux|windows|macos`: manifest `tricky-names.<os>.yaml` of 40 cases each (see Hostile-name corpus) with names stored as base64 bytes | < 1 MB | `data-cache/tricky-names/` | AC-37, 42; Desktop #13, Sync #7 | Names the server accepts are created through the API from a Linux client; names the server must reject are sent through the API and expected to fail; local-origin cases (symlink, junction, local case pairs, long paths) are created by script in the sync root next to canary files and victim files outside/inside the root | Revert VM snapshot / delete container volume; canaries checked before deletion |
| DS-13 S | collide-1000 | `gen_collide.py`: 1,000 local files; the server pre-seed holds 100 paths with identical content (same BLAKE3) and 100 paths with different content from another account/device | about 100 MB | `data-cache/collide/{local,server-seed}` | AC-31; Desktop #8 | Pre-seed via API as device B; connect the folder on device A | Account purge; delete local tree |
| DS-14 H-WIN | evicted-100 | `tools/cfapi-provider` (Windows, Rust/C#): registers a sync root through the Cloud Files API, creates 400 hydrated files and 100 dehydrated placeholders with `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`; its callback logs every `FETCH_DATA` request, which is the oracle for "0 bytes hydrated" | about 10 MB | `data-cache/evicted/` + provider state in the VM | AC-32; Desktop #9 | Run `cfapi-provider create --evicted 100 --normal 400` in the WIN-VM before the daemon starts | Unregister the sync root; revert snapshot |
| DS-15 | editor-saves | `tests/editors/`: `vim.sh` (headless `vim -es`), `keepassxc.sh` (`keepassxc-cli add` / `edit` on a .kdbx), `libreoffice.py` (UNO `store()` headless), `jetbrains-replay.py` (reproduces the safe-write sequence: write temp, rename original aside, rename temp over, delete aside; real IDE on LIN-VM/WIN-VM as validation), `word.ps1` (Word COM `Save` on WIN-VM, needs an Office licence) | < 5 MB | `tests/editors/` + `data-cache/editors/` documents | AC-34; Desktop #11 | Run in the synced folder; the consumer reads file ID and revision count from the API and opens the pre-existing share link | Delete files; revert snapshot |
| DS-16 | redteam-media | `gen_redteam.py`: SVG and PDF renamed `.png`; `.m3u8` text referencing `file:///etc/passwd` and `/proc/self/environ`, named `.mp4`; 30000x30000 PNG (zlib-streamed, about 3 MB compressed, 2.7 GB decoded); 20 GB sparse video (valid ftyp/moov claiming a long duration, sparse mdat); HEIC truncated at 60% of a valid file; plus one valid control image and one valid control video | 20 GB apparent, about 30 MB allocated | `data-cache/redteam/` | AC-25; Rest #12 | API upload (the 20 GB file crosses the wire: needs `blobs-large` and about 3 min at 1 Gbps) | Account purge |
| DS-17 S | uploads-hostile | `gen_hostile.py`: html, svg (with script), xml, js, pdf, a GIF89a/HTML polyglot, a SVG with `onload`; positive controls: jpg, png, webp, gif, mp4 | < 5 MB | `data-cache/hostile/` | AC-47; Sharing #5 | API upload, then download and share-link fetch for each | Account purge |
| DS-18 | vault-file [v1.2] | `gen_vault_files.py`: 100 MiB file, 101 MiB file, empty file, files of exactly 1, 2, 16 chunks at 64 KiB and at 4 MiB | about 210 MiB | `data-cache/vault/` | AC-13, 14, 60; Vault #1-5 | Vault client (web WASM, Android) | Account purge |
| DS-19 | vault-fuzz [v1.2] | `gen_vault_fuzz.py`: takes a valid header written by the product's vault client (`vault-header-gen` test tool), applies 10,000 seeded mutations (single-byte flips at every offset, bit flips, truncation, appended bytes, KDF memory < 64 MiB or > 512 MiB, passes < 3, parallelism != 1, chunk size outside 64 KiB-4 MiB, version byte changes) | about 2 MB | `data-cache/vault-fuzz/` | AC-61; Vault #6 | Fed to the client verification entry point | None (read-only) |
| DS-20 S | oauth-mock | Not a file set: scenarios of `mock-idp` (see External Dependency Mocks) | n/a | `tests/mocks/idp/scenarios/*.yaml` | AC-19, 23, 55, 56, 57, 20; Auth #1-3, #20 | Control API call before each flow | Mock resets per test |
| DS-21 | phone-fixtures | `gen_phone.py`: (a) 5,000 unique JPEG/short MP4 assets (JPEG 1600x1200, 80-300 KB, distinct EXIF dates 2015-2026 and GPS-free) with 100 edited assets: each original is uploaded first, then its file is overwritten in place with edited bytes (same path, same MediaStore asset ID, new content hash), plus 20 control edits saved as new files (new asset ID); (b) 50,000 unique assets (JPEG 640x480 40-120 KB) for the first-sync test; pushed with `adb push` and registered with MediaStore scans | (a) about 1 GB, (b) about 4 GB | `data-cache/phone/` | AC-9, 46, 52, 53; Media #8-11 | `adb push` to `/sdcard/DCIM/Camera` in batches of 500, then `content call` / `am broadcast` media scan; for AC-46 the same 5,000 are first uploaded to the account through the API so the server already holds their hashes; the app is then installed fresh | `adb shell rm`, `pm clear`, uninstall; emulator snapshot revert |
| DS-22 | backup-set | `gen_backup_set.py`: runs a seeded populate script through the API on a stack of release N-1 (20 accounts, about 2,000 files incl. conflict copies, shares, a GC queue with pending blobs, thumbnails, transcodes), then `pg_dump -Fc`, tar of blobs, master key file; manifest of the expected hash set | about 2-3 GB | CI artifact store (per release N-1), unpacked to `data-cache/backup-set/<release>/` | AC-65, 41; Rest #18, Sync #5 | `ctl` restores into a clean host/stack; upgrade starts the N-1 images then switches to N | Remove the clean-host stack |

### Derived data sets (needed by results rows, not listed in data_parameters.md)

| ID | Set | Generator | Used by | Notes |
|----|-----|-----------|---------|-------|
| DS-23 S | small-fixtures | one 1 MiB, one 5 MB, one 1 MiB file for echo, 300 files for mass delete, 500 files for suspend, files of 2 MiB - 1, 2 MiB and 2 MiB + 1 bytes for the single-request threshold (`gen_tree.py`) | AC-5, 8, 18, 29, 35; Sync #1, #3, #4; Desktop #2, #12; Deletion #2 | seeded, per-file BLAKE3 in manifest |
| DS-24 | bg-uploads | 100 files of 100 MiB, uploaded concurrently (100 at once) and restarted as each finishes, for the whole measurement window, from a second account | AC-5, 59 | matches the confirmed definition "100 concurrent 100 MiB uploads"; AC-5 clock: file closed on A to verified complete file on B |
| DS-25 | big-file-1g | 1 GiB dense file | AC-43 range reads, AC-59 delayed upload; Encryption #3 | |
| DS-26 | declared-50g | 100 upload sessions each declaring 50 GiB, no bytes sent | AC-39; Transfer #6 | `perf-client session-open` only |
| DS-27 | slow-writer | script writing a 2 GiB file in 16 MiB steps with 1 s pauses, then idle | AC-18; Desktop #3 | records the last-write timestamp on the VM |
| DS-28 | locked-file | script holding an exclusive lock (`LockFileEx`, share mode none) for 60 s | AC-33; Desktop #10 | Windows only |
| DS-29 | accounts-500 | 500 accounts created through `sunnycloud admin` invite tokens by ctl; 100 concurrent sessions in `sync-mix-v1` (below), 10 min | AC-1; Transfer #8 | mix ratios fixed by AC-1; per-session rates are harness parameters stated below |
| DS-30 | stall-conns | 1,000 TLS/H2 connections that complete the handshake and then send nothing | AC-59; Rest #11 | `perf-client stall --count 1000` |
| DS-31 | update-builds | the same daemon build four ways, each published behind an Ed25519-signed update manifest: valid-signed (test active key A, and a second one signed by the test recovery key R), unsigned, signed by an unpinned test key B, valid but lower version; plus a same-version replay, an expired manifest, a manifest with a non-increasing `serial`, a payload altered after signing and a truncated payload; the two trusted keys (A, R) and the update source URL are build-time constants of the test build | AC-49; Desktop #15 | `mock-update-server` hosts them |
| DS-32 | geoip-test | small test GeoIP database mapping the consumer subnets to two cities | AC-57; Auth #4 | database format follows the product's choice |
| DS-33 | keys | per-run master key, second master key, setup token | AC-43, 23 | generated at stack start, never reused |
| DS-34 S | multiblock-100 | `gen_slices.py`: 100 files, each a contiguous slice of DS-09 (dense big-file) at seeded, non-overlapping, 2 MiB-aligned offsets; sizes seeded between 4 MiB + 1 B and 48 MiB, so every file has at least 3 blocks and a partial last block; total about 2.5 GiB; manifest holds offset, length, whole-file and per-block BLAKE3 | AC-38; Transfer #5 | Dense random content means a zero-filled block is never legitimate, so any zero or missing block is detectable by hash. 100 concurrent uploads, `kill -9` of the api at three seeded points (about 25%, 50%, 75% of aggregate bytes), restart, re-list; surviving clients retry |
| DS-35 S | compliance-edge | `gen_video.py edge`: seven 20 s clips, each equal to DS-07 except one property: video 10.5 Mbit/s, 1920x1088 (short side over 1080), High 10 profile (yuv420p10), `moov` at the end, BT.2020 HLG, HEVC, MP3 audio; plus two controls that must pass untouched: 9.5 Mbit/s, and portrait 1080x1920 (orientation independent bound) | AC-11 (boundary of "compliant") | six are re-encoded; the `moov`-at-the-end clip (codecs compliant) is losslessly remuxed, not re-encoded; the controls are never re-encoded |
| DS-36 | clip-1gib | `gen_video.py clip-1gib`: the DS-05 recipe (1080p SDR H.264 above 10 Mbit/s, so a transcode job is planned) cut at a whole GOP, then a `free` box pads the MP4 to exactly 1,073,741,824 bytes; whole-file and per-block BLAKE3 in the manifest | AC-72 (worker fetch of a 1 GiB blob) | stays a valid MP4; the planner enqueues a video job for it |

## Hostile-name corpus (DS-12)

Derived from the confirmed policy (AC-37): the **server rejects** names that contain `/`, NUL, are exactly `.` or `..`, or contain control characters or bidi-control characters, and **accepts everything else**, including Windows-reserved names, backslashes, colons, trailing dots and spaces, zero-width characters, case variants (but not case-insensitive duplicates, AC-42) and NFC/NFD forms as distinct names. A client that cannot represent an accepted name on its OS saves it under an escaped name and reports it.

Outcome codes: **SR** server rejects with HTTP 4xx, the name is absent from the listing and never reaches a client; **AS** stored as-is under the original name; **ES** stored under an escaped name and reported (`%XX` escaping: each character the client cannot represent becomes `%` plus the uppercase hex of its UTF-8 bytes, so `a:b` is `a%3Ab`; original name in the report, content hash equal, no re-upload of the escaped name on the next pass); **SK** not followed, not uploaded as content, reported as skipped, nothing written through; **CC** conflict copy, nothing overwritten.

40 cases per OS; the case list per category is the same on every OS, only the client outcome differs. The manifest `tricky-names.<os>.yaml` stores raw name bytes (base64), origin (server-sent, local, or both), the canary or victim file it must not touch, and the expected outcome code.

| # | Category (cases) | Example names | Server | Linux | Windows | macOS [v1.1] |
|---|------------------|---------------|--------|-------|---------|--------------|
| 1 | Whole-name traversal, separators, NUL, control, bidi (10) | `.`, `..`, `a/../../x`, `/etc/x`, `a<NUL>b`, `a<LF>b`, `a<TAB>b`, `a<U+007F>b`, `a<U+202E>txt.exe`, `a<U+2066>b` | SR | SR; a local file with such a name (LF, TAB, bidi) is refused by the server, reported once, not retried in a loop | SR | SR (local variants as Linux) |
| 2 | Backslash, drive and UNC forms (4) | `..\x`, `a\..\..\x`, `C:\x`, `\\host\s\x` | accepted | AS (backslash is an ordinary character) | ES (backslash and colon are not representable); no write outside the root | AS |
| 3 | Windows device names (4) | `CON`, `NUL.txt`, `COM1`, `lpt1.log` | accepted | AS | ES | AS |
| 4 | Trailing dot or space, wildcard characters (3) | `file.`, `file `, `a\|b?*"` | accepted | AS | ES | AS |
| 5 | Alternate data stream syntax (3) | `f.txt:hidden`, `f.txt::$DATA`, `f.txt:s:$DATA` | accepted | AS (literal colon) | ES; no stream is created on `f.txt`, which stays unmodified (`Get-Item -Stream *` shows only `:$DATA`) | AS |
| 6 | Zero-width character (1) | `a<U+200B>b` | accepted | AS | AS | AS |
| 7 | Case collision (3) | server pairs `A.txt`/`a.txt` and `README`/`readme`; local `A.txt` + `a.txt` together | second create rejected or renamed (AC-42) | local pair: both files untouched, second rejected or renamed by the server and reported | server `a.txt` vs local `A.txt` with different content: CC, 0 overwritten (a local pair cannot exist) | as Windows |
| 8 | Unicode normalisation pairs (3) | `e<U+0301>` vs `<U+00E9>`; `<U+212B>` vs `<U+00C5>`; precomposed vs jamo Hangul | both accepted as distinct names | AS both | AS both | first AS, second ES (APFS treats the pair as one name) |
| 9 | Symlink, junction, link races (6) | link in root to an outside directory; link to an outside file; server folder `link/` with file `link/x` over a local link; parent replaced by a link between listing and write; dangling link; link loop | accepted | SK for all six; target content never uploaded; canary file outside the root unchanged | same with directory junction, file symlink (Developer Mode on the VM), junction race, dangling link, junction to its own parent | SK for all six (symlinks) |
| 10 | Length (3) | 255-byte name; 256-byte name; 300-character path (6 nested directories of 50 characters) | accepted | AS; ES; AS | AS; ES; AS or ES depending on long-path support (invariants below must hold either way; the outcome is fixed when the Windows long-path decision is written) | AS; ES; AS |

Invariants asserted for every case on every OS: 0 files created or modified outside the sync root (canary directory hash manifest unchanged); 0 pre-existing files overwritten (victim files unchanged); the daemon keeps running and syncs the rest of the tree; a second sync pass is stable (no upload of an escaped name, no rename loop); every skipped or escaped item is listed in the daemon's report. The escaped name is asserted exactly: every replaced character appears as `%XX` (decision 9, helper 06); only the extra marker for Windows device names is taken from helper 06 when it is final, until then the test checks that the escaped name differs from the original, matches `%XX` where a character was replaced, and exactly one local file with the right content hash exists.

Excluded: names that are invalid UTF-8 or contain lone surrogates cannot be expressed in the JSON API and are not part of the 40.

## AC-1 workload profile `sync-mix-v1` (DS-29)

Mix fixed by AC-1: 70 idle long-poll sessions (`wait=30`, no changes), 20 small-file-edit sessions, 10 large-transfer sessions, 500 registered accounts (the other 400 accounts are registered and idle), 10 minutes, aggregate 1 Gbps on `shape-perf`, REF-SRV with the release image. Harness parameters (not in the AC): small-file sessions edit a random file of 1 KiB to 1 MiB (log-uniform) about every 2 s and read the change back on a second connection; large-transfer sessions alternate uploading and downloading 1 GiB DS-25-style files back to back, and their count is tuned inside the 10% so the aggregate reaches 1 Gbps. Pass values are in RR Transfer #8 (0 failed sessions, 5xx <= 0.1%).

## Data Isolation Strategy

- One account per test (or per parametrised case) created through a single-use invite token by `e2e-ctl`; folder prefix `t-<test id>-<random>`; no test reads another test's account.
- Immutable inputs: `/data` is read-only; generated files are never edited in place by a test (copy into the sync root or a temp dir first).
- Stack state: group `P` shares one stack; group `X` runs on a stack restored from the golden snapshot (`golden`, `golden+listing`, `golden+accounts-500`, `golden+empty` for AC-23 before bootstrap). The restore also resets the XFS image and the mock-idp state.
- Clients: desktop containers get fresh sync volumes per test; VMs revert to a named snapshot (`clean`, `tree-100k`, `evicted-100`) before each H-* test; emulator reverts to an AVD snapshot (`fresh`, `phone-5000`, `phone-50000`); physical devices get `pm clear` and `rm` of fixtures, and a leftover check lists `DCIM/Camera` before and after.
- Time: any test using clock skew resets the offset to zero in teardown and the next test fails fast if the offset file is non-zero.
- Leak check: bytes on the blob volume and row counts (via ctl) are compared before and after each group; a difference is reported as a FAIL of the group's last test.
- Parallelism is limited by the group rules in `environment.md`.

## Input Data Mapping

| Input Data File | Source Location | Description | Covers Scenarios |
|-----------------|----------------|-------------|-----------------|
| `data_parameters.md` | `_docs/00_problem/input_data/data_parameters.md` | The 22 named sets (DS-01 to DS-22; `big-file` is two sets) | All rows above |
| `results_report.md` | `_docs/00_problem/input_data/expected_results/results_report.md` | Expected results by area | See Expected Results Mapping |
| `manifest.json` per set | `tests/data-cache/<set>/manifest.json` (generated) | Sizes, hashes, per-set expectations | All generated sets |
| `sources.lock` | `tests/data/sources.lock` (to create) | Pinned URLs and SHA-256 of free-licence base media | DS-03, 04, 05, 06, 07, 08 |
| `mix.yaml` | `tests/data/mix.yaml` (to create) | Composition of DS-08 | AC-44, 45 |

## Expected Results Mapping

Test IDs are assigned by the test-spec files; they cite the `RR` row that holds the expected value. Every set has at least one row.

| Input Data | RR row | Expected result | Comparison | Tolerance |
|-----------|--------|-----------------|------------|-----------|
| DS-09 (5 GiB) | Transfer #1 | Hash equal; <= 10 MiB re-sent after drop at 50% | exact; threshold_max | <= 10 MiB |
| DS-09 | Transfer #2, #3 | >= 112 MB/s up and down, hash equal | threshold_min; exact | calibrated goodput recorded |
| DS-10 (50 GiB) | Transfer #4, Web #1, #2 | Hashes equal; finish <= 2 s; heap <= 150 MB up / 200 MB down; <= 10 MiB re-sent | exact; threshold_max | as stated |
| DS-01 | Transfer #7 | 10,000 files <= 60 s | threshold_max | 60 s |
| DS-26 | Transfer #6 | Refusals beyond cap and quota; free space >= floor | exact; threshold_min | N/A |
| DS-29 | Transfer #8 | 0 failed sessions; 5xx <= 0.1% | exact; threshold_max | 0.1% |
| DS-23, DS-24 | Sync #1, #3, #4 | Content on B equals A within 5 s p95; conflict copy count 1; edit beats delete | exact; threshold_max | 5 s p95 |
| DS-02 | Sync #2 | Listing <= 300 ms p95 | threshold_max | 300 ms |
| DS-22 | Sync #5, Rest #18 | Devices converge after `resync_required`; hash set equal after restore/upgrade | exact | converge <= 60 s |
| DS-12 (Linux) | Sync #7, Desktop #13 | Second `a.txt` rejected or renamed; 0 writes outside root, 0 overwritten files | exact | 0 / 0 |
| DS-23 | Deletion #1, #2 | Delete hold with prompt; 200 files return after "no" | exact | N/A |
| DS-23 (one file) | Deletion #3, #4, #5 | Blob present day 6, gone after day 7; restore returns original hash; account deletion leaves 0 rows, 0 blobs | exact | retention 7 d |
| DS-11 | Desktop #1 | <= 1% CPU, <= 100 MB, scan <= 60 s | threshold_max | as stated |
| DS-23, DS-27 | Desktop #2, #3 | 0 re-uploads; upload starts >= 2 s after last write | exact; threshold_min | 2 s |
| (settings window, no data file) | Desktop #4-#7, #14 | Setting applied <= 5 s; window <= 1.0 s p95; native dialog; sync without tray; IPC refused | threshold_max; exact | as stated |
| DS-13 | Desktop #8 | 0 lost, 0 identical bytes uploaded, exactly 100 conflict copies | exact | N/A |
| DS-14 | Desktop #9 | 0 B hydrated (callback log), 100 reported skipped, 0 deletions | exact | N/A |
| DS-28 | Desktop #10 | <= 1% CPU; upload <= 10 s after release; never reported deleted | threshold_max; exact | as stated |
| DS-15 | Desktop #11 | Per editor: 1 new revision, same file ID, 0 conflict copies, 0 deletes, share link opens | exact | N/A |
| DS-23 (500 files) | Desktop #12 | Converge <= 60 s after wake | threshold_max | 60 s |
| DS-31 | Desktop #15 | All 8 bad builds rejected (two good builds, A and R, installed) | exact | N/A |
| DS-03 | Media #1 | Thumbnail ready <= 5 s after upload completes (p95); orientation applied | threshold_max; exact | 5 s |
| DS-04 | Media #2 | Bulk done <= 15 min; interactive p95 <= 5 s | threshold_max | 15 min / 5 s |
| DS-05, DS-06 | Media #3, #4 | Playable H.264 High 1080p AAC MP4 in <= 5 min; HLG tone-mapped SDR with correct tags in <= 20 min | threshold_max; exact | as stated |
| DS-07 | Media #5 | Derived transcode count 0 | exact | N/A |
| DS-08 | Media #6, #7 | Derived/original <= 20%; ffmpeg killed <= 90 s after last progress and retried on CPU; drift <= 40 ms | threshold_max; exact | as stated |
| DS-21 | Media #8-#11 | Pixel arrival <= 60 s; 0 media bytes on reinstall; <= 1% re-hash, 0 duplicates, RSS <= 150 MB over 10 kills; "N photos visible, tap to allow more" | threshold_max; exact | as stated |
| NVENC worker (hardware) | Media #12 [v1.1] | GPU used, then CPU fallback with valid output | exact | N/A |
| (one file, share) | Sharing #1-#4 | 200 and equal bytes in <= 2 s; revoked = 404 within 5 s; revoked/expired/missing identical; DB holds hashes only | exact; threshold_max | median delta <= 1 ms |
| DS-17 | Sharing #5 | `attachment` + `nosniff` + sandbox CSP for non-allowlisted; allowlisted inline with `nosniff` | exact | 0 inline renders |
| DS-09, DS-25 | Encryption #1-#3 | No marker substring in the blob file; rotation leaves blobs unchanged; range reads correct | exact | N/A |
| DS-20 | Auth #1-#3, #20 | No account without token; 0 accounts for unverified email; all negative callbacks rejected; each provider signs in | exact | 12 of 12 |
| DS-32 | Auth #4 | Page shows both locations and warning; code expires <= 10 min | exact; threshold_max | 10 min |
| (device grant, session, OpenAPI) | Auth #5-#10 | 403 per forbidden call; 429 + Retry-After; stream cut <= 5 s; CSRF 403; refresh race; IDOR matrix 0 successes | exact | as stated |
| DS-30, DS-25 | Auth #11 | RSS increase <= 200 MB; 1 GiB upload delay <= 10% | threshold_max | as stated |
| DS-16 | Auth #12 | Every sample fails safely; no credentials, blob mount or network in decoder | exact | 0 exposures |
| DS-09 (one blob) | Auth #13 | One flipped bit found by the next scrub, throttled | exact | N/A |
| CI jobs, release image | Auth #14, #15 | Build fails on unreviewed or stale-ignore advisory; 0 HIGH/CRITICAL, rebuilt <= 7 days | exact | N/A |
| mobile app, old client | Auth #16, #17 | Plain HTTP refused; clear upgrade message | exact; substring | N/A |
| clean host | Auth #19 | Working server <= 30 min | threshold_max | 30 min |
| DS-18 | Vault #1-#5 [v1.2] | Ciphertext differs; decrypts to original hash; tampered variants rejected with no output file; 101 MiB rejected; 1,000/1,000 swaps rejected | exact | as stated |
| DS-19 | Vault #6 [v1.2] | 0 of 10,000 mutations and bad parameters accepted | exact | 0 accepted |
| web vault page | Vault #7 [v1.2] | No `unsafe-inline`/`unsafe-eval`; 0 third-party hosts; no key in heap | exact | N/A |
| DS-12 (all OS) | Desktop #13, Thresholds #11 | Per-category outcome in the Hostile-name corpus table; server rejects `/`, NUL, `.`, `..`, control and bidi-control names, accepts the rest; unrepresentable names escaped and reported | exact | 0 writes outside root, 0 overwritten |
| DS-34 | Transfer #5 | Every committed file equals its manifest hash; no missing or zero-filled block; uncommitted files absent or completed after retry | exact | 0 bad files |
| AC-68..80 (DS-01, 22, 23, 34, 36, 20) | none in `results_report.md` | The expected values are the numbers in the AC text itself (for example 50 concurrent commits, 1,000 operations, 5 s, exactly one account); the scenarios state them | exact; threshold_max | as stated in each scenario |
| DS-35 | Media #5, Thresholds #7 | Six near-miss clips re-encoded; the index-at-end clip remuxed, not re-encoded; the 9.5 Mbit/s and portrait 1080x1920 controls untouched | exact | re-encodes 0 for the remuxed clip and both controls |
| DS-09 (derived blobs of DS-03, DS-05) | Encryption #1, gap 9 | Thumbnails and transcodes are encrypted too: no blob file on the volume starts with a JPEG, WebP or MP4 signature | exact | 0 files |
| DS-12, DS-20, DS-21, DS-22, DS-05, DS-09, DS-25, DS-11, scrub and worker tests | Thresholds #1-#12 (results_report, section added after owner confirmation) | 4 KiB range read <= 5 ms p95; NVENC >= 1.3x (v1.1); initial hashing <= 25% of one core; scrub keeps transfers >= 90% of baseline; decoder limits photos 2 CPU/2 GiB/120 s, video 4 CPU/4 GiB/40 min; vault unlock <= 1 s native / <= 2 s browser (v1.2); account deletion <= 15 min; limits 5/20/50, 10 per 10 min, 100/min, 10/min with 429 + Retry-After; after DB restore local-only files upload and server-only files download | threshold_max / threshold_min; exact | as stated |

## External Dependency Mocks

All mocks are deterministic: same scenario and seed give the same bytes, identifiers and timing offsets. Clock-dependent values (`iat`, `exp`) come from the mock's own clock, which a control call can set.

| External Service | Mock/Stub | How Provided | Behavior |
|-----------------|-----------|-------------|----------|
| Generic OIDC provider (Google through OIDC, Authelia-like) | `mock-idp` OIDC module | Docker service, alias `idp.test`, control API `POST /__control/scenario` | Discovery, JWKS (rotating test keys), authorize, token (PKCE checked), userinfo; scenarios: `valid`, `unverified-victim-email`, and the negative callbacks below |
| GitHub OAuth2 | `mock-idp` GitHub module | same | `/login/oauth/authorize`, `/login/oauth/access_token`, `/user`, `/user/emails` with a `verified` flag per address |
| Sign in with Apple [v1.1] | `mock-idp` Apple module | same | ES256 client-secret JWT validated against the test team key; no scopes, no PKCE; relay-email and unverified-email cases |
| Negative callbacks (AC-56) | scenario list | `scenarios/neg-*.yaml` | missing state, replayed state, foreign state, wrong provider, wrong nonce, wrong audience, wrong issuer, absolute return URL, foreign Host header, reused one-time code, wrong code verifier, expired state (12) |
| Update source (AC-49) | `mock-update-server` | Docker service `update.test` | Serves DS-31 builds and their signed manifests (GitHub-Releases-shaped layout); the SUT sync server is separately made to advertise a build to prove it is ignored |
| GeoIP (AC-57) | DS-32 database | file mounted into the API test config | Fixed city per test subnet |
| Cloud Files provider (AC-32) | `cfapi-provider` | Windows tool | Placeholders with an auditable hydration log |
| Live providers (S8) | none, real accounts | owner-registered GitHub OAuth app, Google OIDC client, Apple (v1.1) | `H-LIVE` smoke only, secrets from the lab secret store |
| Email | none | not applicable | The system sends no email |
| Object store, Redis | none | not applicable | Not part of the design |

## Data Validation Rules

| Data Type | Validation | Invalid Examples | Expected System Behavior |
|-----------|-----------|-----------------|------------------------|
| Names in a folder | Unique case-insensitively, case preserved | `A.txt` then `a.txt` | Second create rejected or renamed (RR Sync #7); first untouched. NFC and NFD forms of one name are distinct names and both accepted |
| Name characters | Server rejects `/`, NUL, `.` and `..` as whole names, control characters and bidi-control characters; accepts everything else (Windows-reserved names, backslash, colon, trailing dot/space, zero-width characters) | `a/b`, `a<NUL>b`, `..`, `a<U+202E>b`, newline | HTTP 4xx, name absent from the listing; accepted names stored byte-exact on the server |
| Upload block | 2 MiB fixed blocks, BLAKE3 per block and file | Wrong block hash, wrong length, out-of-order commit | Rejected; no committed file with a missing or zero-filled block |
| Upload session declared size | Counts against quota and session cap | 100 sessions of 50 GiB | Refused beyond cap/quota; free space >= floor |
| Cursor | `epoch.seq` | Unknown epoch, seq below purge horizon | `resync_required` |
| API version | `/api/v1`; minimum client version | Client below minimum | Clear upgrade message |
| OAuth callback | State, nonce, audience, issuer, PKCE verifier, one-time code | Each of the 12 negative cases | Rejected, no session, no account |
| Share token | >= 128 bits, stored as hash | Missing, expired, revoked token | Identical response (status, body, headers, median timing within 1 ms) |
| Content type on serve | Inline only for the allowlist | html, svg, xml, js, pdf, polyglot | `attachment`, `nosniff`, sandbox CSP |
| Vault header [v1.2] | KDF memory 64-512 MiB, passes >= 3, parallelism 1, chunk 64 KiB-4 MiB, authenticated | Any out-of-range value or altered byte | Rejected by the client |
| Vault file size [v1.2] | <= 100 MiB (configurable) | 101 MiB | Rejected with a clear message |
| Device-grant user code | TTL <= 10 min | Use after expiry | Rejected |
| Update package | Signed by build-time key, version above current | Unsigned, wrong key, lower version | Rejected |

## Missing or hard-to-produce data (real hardware, accounts or open decisions)

Resolved by the owner's confirmed gap decisions and removed from this list: compliant-video definition, hostile-name policy, the 12 OAuth callbacks, AC-1 mix, the 100-background-uploads definition, edited-photo link rule, REF-SRV size, and the server test clock.

| # | Item | Why it is hard | Needed by | Proposal |
|---|------|----------------|-----------|----------|
| 1 | Real Pixel, Samsung and Xiaomi devices (models, OS versions) with battery optimisation exempt, plus a stable Wi-Fi | Hardware; Xiaomi autostart permission; vendor background killers | AC-9, 52 (RSS), 53 | Owner supplies devices; EMU covers functional parts only |
| 2 | Windows cloud-evicted files from real OneDrive (a Microsoft account) and iCloud (macOS, v1.1) | Real placeholders need real accounts and clients | AC-32 | `cfapi-provider` gives a controllable equivalent; one real OneDrive sample as validation |
| 3 | 50 GiB and 20 GB files, 200 GiB XFS volume, 50 GiB free on browser hosts | Disk and time (about 8 minutes per 50 GiB pass at 112 MB/s; browsers triple it) | AC-4, 25, 50, 51 | Sparse-hybrid generator (DS-10); self-hosted runner; nightly only |
| 4 | Freely licensed 4K HLG phone footage | None known; HLG is synthesized by conversion | AC-11 ref B | Synthesized clip; owner may supply one real CC0 HLG clip to validate tone-mapping tags |
| 5 | Composition of the 100 mixed clips | The 20% derived-storage bound depends on the mix | AC-44 | `mix.yaml` proposed above; owner to approve |
| 6 | Previous-release images for the upgrade test | v1.0 has no predecessor | AC-65 | Use the last release candidate as N-1 until v1.0 ships; later the previous tag |
| 7 | GeoIP database and the product's lookup source | Not chosen | AC-57 | DS-32 once chosen |
| 8 | Word licence on a Windows VM; JetBrains IDE licence/community edition; LibreOffice, KeePassXC installs | Licences and GUI automation | AC-34 | Owner supplies Office; JetBrains Community; replay scripts as fallback |
| 9 | Real provider accounts and a domain with DNS API access | Accounts and ownership | S8, `lan` profile, physical Android | Owner registers GitHub OAuth app, Google OIDC client, a domain; Apple only for v1.1 |
| 10 | Reference hardware specifics (REF-SRV, REF-CPU4, REF-NV, weak 2019 laptop, Mac, iPhone) | Owner-held hardware | AC-1, 2, 10, 11, 16, 28, Safari, iOS | Owner states models and CPU/NVMe of REF-SRV (sizing is fixed: 8 physical cores, 32 GB, NVMe, 1 GbE); they are recorded in every report |
| 11 | Windows exclusive-lock behaviour on Linux and macOS | Locks are advisory there | AC-33 | Windows only; others reported not applicable |
| 12 | Web 50 GiB file input in Firefox and WebKit through Playwright | Path-based input may buffer the whole file in those browsers | AC-50 | Spike first; fall back to a manual-driven run on the lab host, labelled as such |
| 13 | Windows long-path behaviour for the 300-character path, and the exact marker for escaped Windows device names (the `%XX` format itself is decided) | Long-path support depends on the OS setting; device-name marker is pending in helper 06 | AC-37 | Tests assert content hash, single file, report entry and stable second pass; exact names for `%XX` cases |
| 14 | Whether overwriting a file in place on Android keeps the MediaStore asset ID on every device/OS version | The confirmed link rule depends on it | AC-46 | Spike on emulator and the three phones; if an OEM changes the ID, that device is reported, not worked around |
| 15 | Windows Cloud Files and symlink rights on the lab VM (Developer Mode for file symlinks) | VM image setup | AC-32, 37 | Part of the WIN-VM image |

## ACs and RESTRICT items this data supports

**ACs**: AC-1 (DS-29), 2, 3, 7, 43 (DS-09, 25), 4, 50, 51 (DS-10), 5 (DS-23, 24), 6 (DS-02), 8 (DS-23), 9, 46, 52, 53 (DS-03, 21), 10 (DS-03, 04), 11 (DS-05, 06, 07, 35), 12 and 47 (DS-17, share fixtures), 13, 14, 60 (DS-18) [v1.2], 15 (DS-11), 18 (DS-23, 27), 19, 20, 23, 55, 56, 57, 58 (DS-20, 32), 21, 59 (DS-24, 25, 30), 24 (DS-09), 25 (DS-16), 27 (DS-23), 29 (DS-23), 30 (DS-23), 31 (DS-13), 32 (DS-14), 33 (DS-28), 34 (DS-15), 35 (DS-23), 68 (DS-34, 01), 69 (DS-01), 70 (generated 64 KiB files), 71 (DS-03, 05), 77 (DS-23), 72 (DS-36), 73 (DS-22), 74 (DS-01 at fixed size), 75 and 76 (DS-20), 80 (DS-01, 34, 09), 37 and 42 (DS-12), 38 (DS-34), 40 (DS-01), 39 (DS-26), 41 and 65 (DS-22), 44 and 45 (DS-07, 08), 49 (DS-31), 61 (DS-19) [v1.2]. Not data-driven: AC-16, 17, 22, 26, 48, 54, 62, 63, 64 (use the environment only) and AC-28 (needs a GPU worker, DS-05 as input).
**RESTRICT**: RESTRICT-07 (names, XFS), 08 (marker at-rest check), 09 (cursor, tombstones), 10 (retention fixtures), 11 (block sizes and single-request threshold: the 2 MiB - 1, 2 MiB and 2 MiB + 1 files in DS-23), 13 (DS-16 red-team set), 17 to 21 (DS-20), 23 (DS-18, DS-19) [v1.2], 24 (DS-13, 14, 23), 25 (DS-12 corpus) and 31 (DS-22).
