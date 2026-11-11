# Batch Report

**Batch**: 43
**Tasks**: AZ-3930 done (three engines); decisions recorded for AZ-3851, AZ-3849 and the decoder image checks. Documents only, no code.
**Date**: 2026-11-11

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3930_vite_wasm_spike | Done | The production build (Vite 8, wasm-bindgen 0.2.130 `--target web`) loads the WASM in a module Worker through the hashed `.wasm?url` import. The exact CSP of 16 section 5 works in Firefox 159, Chromium 158 and Playwright WebKit (Linux, not Safari) and every engine needs `'wasm-unsafe-eval'` (without it 5 of 5 runs fail). Probe wasm 30,918 B (12,765 gzip, 10,723 brotli); first instantiate in the Worker: 6 ms Firefox, 3 ms Chromium, 19 ms WebKit. Safari NOT-RUN(HW-DEVICE) |
| AZ-3934_too_many_files_spike | Done (batch 42), WebKit added | WebKit is the lightest engine: 100,000 files about 250 MB and 1.5 s, 500,000 files about 1.1 GB and 7.5 s; Chromium (11.4 s at 100,000) sets the limit; the confirmed limit of 100,000 stands |

Owner decisions applied after batch 42: HLG poster-frame SSIM must be >= 0.95 against the harness render of an equivalent chain (G-10-6 resolved); AZ-3849 AC-1 reworded (the 20 GB sparse MP4 is held by the limits, not refused by a list); the decoder image build checks (no test stubs, no `test-freeze`) added to `containerization.md`.

## Code Review Verdict: PASS

Documents only. AZ-3930 archived; Jira `In Testing`.

## Test Suite

Not run: no code changed (last full run in batch 37: 2762 passed, 0 failed, 9 ignored).

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | The build scan rule of 16 section 5 ("a non-self URL fails the build") needs an allowlist: the Svelte runtime holds eight URL strings that are never requested | AZ-3930 | Noted in 16 description | unclear |
| 2 | Safari is not Playwright WebKit; the Safari rows stay NOT-RUN(HW-DEVICE) until a Mac exists | AZ-3930, AZ-3934 | Labelled | unclear |
| 3 | The probe wasm is the probe crate only (about 31 KB); the real `sync-core-wasm` will be larger | AZ-3930 | Repeat when E-26/c06 builds it | unclear |
| 4 | `/tmp` is a shared tmpfs: a 500,000-file tree filled its inodes once during the run; the worker cleaned its own tree and the state is back to 23% inodes | AZ-3934 | Noted | clear |
| 5 | Scratch left for the owner or a later cleanup: `scratchpad/web-spike` (725 MB), the mounted test images under `/develop/sc-fs-spike/` (unmount.sh needs sudo) and `/tmp/claude-1000/kp.*` | | Not deleted by the agent | unclear |
| 6 | Items open from batches 29-42 | earlier batches | | unclear |

## Commit

`[AZ-3930] Web WASM packaging spike: three engines` — Loop: 1

## Next Batch

Ready but blocked on hardware: AZ-3799, 3881-3884, 3886, 3921, 3985, 3987, 3988, 4005 and AZ-3893; AZ-3849 and AZ-3850 wait for REF-CPU4 and real clips; AZ-4004 waits for the owner. 234 tasks left.
