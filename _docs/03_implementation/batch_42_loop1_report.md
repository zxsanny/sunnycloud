# Batch Report

**Batch**: 42
**Tasks**: AZ-3885, AZ-3851, AZ-3934 done; AZ-3849, AZ-3850, AZ-3930 partly done. Documents only, no code. Measurements on a developer workstation (Ubuntu 26.04, Ryzen 9 3900XT, kernel 7.0.0-42), all informational.
**Date**: 2026-11-11

The owner installed libvips, libheif, gnome-keyring and libsecret tools; Firefox, Chromium (build 1254) and WebKit were downloaded for Playwright 1.65.0 (user level). Three workers ran in parallel on disjoint files; the parent applied `tests/spec-gaps.md`, checked the diffs and kept tracker, commit and push.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3885_linux_keyring_systemd_spike | Done | keyring 4.2.0 measured in unlocked, locked, no-keyring, no-bus and no-Secret-Service contexts (also as a `systemd-run --user` unit) in private throwaway sessions. Detection rule: `Entry::store_status()` with a 5 s bound; an error or timeout selects the token file, a present but locked keyring never does. A locked keyring with an unanswered prompt blocks `set` with no timeout, so calls are bounded to 30 s. File `$XDG_STATE_HOME/sunnycloud/token-<account id>` (0600 in 0700), fixed WARN text; G-12 resolved |
| AZ-3851_decoder_watchdog_hook_ssim_spike | Done | Feature `test-freeze` (SIGUSR1 blocks only the main loop; proof of concept aborted after 3.0 s; SIGSTOP did not abort); G-10-4 resolved. Measured SSIM of tone-mapped HLG poster frames (synthetic clip): 0.85 to 0.91 against the SDR master, 0.995 or more against a harness render of an equivalent chain; no tolerance chosen (owner item G-10-6) |
| AZ-3934_too_many_files_spike | Done | Limit confirmed by the owner: `web.max_selection_files` = 100,000, refusal `TooManyFiles`; PT-16-4 restated (at 100,000 files at most 1.5 GB browser memory and 15 s to the first upload; up to 10,000 files 150 MB and 5 s). Measured in Firefox 159 and Chromium 158 (500,000 files crashes Firefox) |
| AZ-3849_decoder_s5_whitelist_hdr_spike | **Partly, stays in todo** | Whitelists (formats `mov,matroska,avi,mpegts`; codecs `h264,hevc,vp9,libdav1d,aac,opus,mp3float,mov_text,webvtt`), the `-progress pipe:1` sink under `-protocol_whitelist file` and the HDR chain order (downscale first, then zscale/hable, about 1.4 s per second of media informational; proposed luma tolerance 2.0 code values) are recorded. Missing: the AC-3 budget on REF-CPU4, real phone clips, the real DS-06 base |
| AZ-3850_decoder_s4_heic_stubs_spike | **Partly, stays in todo** | `vipsthumbnail` option strings, the 320 px HEIC thumbnail by full decode, the `test-stubs` mechanism (G-10-1 resolved) and the 30 s base-clip manifest requirement (G-10-3 stays open until E-31/c21) are recorded. Missing: HEIC pair timings on REF-CPU4 and real iPhone HEIC files |
| AZ-3930_vite_wasm_spike | **Partly, stays in todo** | Vite 8 + wasm-bindgen 0.2.130 `--target web`: the Worker imports the hashed `.wasm?url`; the exact CSP of 16 section 5 works with `'wasm-unsafe-eval'` and fails without it in both engines; probe wasm 30,918 B (12,765 gzip, 10,723 brotli); first instantiate in the Worker 6 ms (Firefox) and 3 ms (Chromium). Playwright WebKit not run: `libmanette-0.2-0` is not installed; Safari NOT-RUN(HW-DEVICE) |

## Code Review Verdict: PASS

Documents only. Three specs archived to `done/`; Jira `In Testing` for AZ-3885, AZ-3851, AZ-3934. The decoder leftover is `_docs/_process_leftovers/2026-11-11_az-3849-3850-3851-decoder-spikes.md`.

## Test Suite

Not run: no code changed (last full run in batch 37: 2762 passed, 0 failed, 9 ignored).

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | Which SSIM reference counts for HLG poster frames: the SDR master (0.85 to 0.91) or an equivalent harness render (0.995 or more)? The 0.95 tolerance only fits the second | AZ-3851, G-10-6 | Owner item | unclear |
| 2 | AZ-3849 AC-1 says every DS-16 video item is refused; the 20 GB sparse MP4 is a valid MP4 held by limits | AZ-3849 | Reword needed | unclear |
| 3 | `containerization.md` build checks should gain the stub-free and `test-freeze`-free checks; whether `avi` and `mpegts` stay in the format list is open | AZ-3849, AZ-3850 | Not edited | unclear |
| 4 | The 5 s detection and 30 s call bounds, a second file check on a missing keyring entry, and `PermissionTooOpen` for a wide token directory are additions of the keyring worker | AZ-3885 | Easy to drop | unclear |
| 5 | The build scan rule of 16 section 5 ("a non-self URL fails the build") needs an allowlist: the Svelte runtime holds eight URL strings that are never requested | AZ-3930 | Noted in 16 description | unclear |
| 6 | WebKit and Safari: `libmanette-0.2-0` is not installed (needs the owner's apt); Safari needs a Mac | AZ-3930, AZ-3934 | AZ-3930 stays in todo | unclear |
| 7 | Items open from batches 29-41 | earlier batches | | unclear |

## Commit

`[AZ-3885] [AZ-3851] [AZ-3934] [AZ-3849] [AZ-3850] [AZ-3930] Keyring, decoder and web spikes` — Loop: 1

## Next Batch

Ready with a plan: AZ-3930 once `libmanette` is installed. Everything else ready is blocked on hardware: AZ-3799, 3881-3884, 3886, 3921, 3985, 3987, 3988, 4005 and AZ-3893; AZ-3849 and AZ-3850 wait for REF-CPU4 and real clips; AZ-4004 waits for the owner. 235 tasks left.
