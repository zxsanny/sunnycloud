# Batch 126, loop 1: AZ-3858 probe latency bounds asserted on the container (AZ-3858 complete)

**Batch**: 126
**Tasks**: AZ-3858_decoder_probe (AC-1 latency bounds; AC-2 to AC-5 were proven in batch 110)
**Date**: 2026-11-26

## What was found

The open gap of AZ-3858 was AC-1's latency: a 12 MP photo probe within 50 ms, the 30000x30000 header-only PNG within 100 ms. The host-process case measured 250 to 310 ms in batch 110 and was put down to a loaded host.

The cause is the runner, not the load. Measured today on this host:

- `vipsheader` alone on a 12 MP JPEG: 30 ms.
- The runner's probe, run as a host process: 191 to 207 ms (best of five), the same for a 640x480 GIF as for 12 MP. A fixed cost.
- One read of the whole process table: 20 ms for the 1,021 processes of this host. The process guard reads it for every group check (the group-alive check after the tool ends, the wait for the group to end, the memory poll, the kill-all sweep after the job). A probe makes two `vipsheader` runs, so it does five to seven such reads.
- The same probe in the decoder container (`sunnycloud-decoder:e2e`, its own pid namespace, as deployed): 24 to 28 ms median, the huge PNG 26 ms.

No deploy file shares the host's pid namespace with the decoder (`deploy/` has no `pid:` key), so the container numbers are the product's.

## What was built

- IT-10-4 (`src/decoder/tests/container/it_media_image.rs`) asserts the bounds: the median of five probes is at most 50 ms for each 12 MP photo (JPEG, HEIC, PNG, WebP) and at most 100 ms for the 30000x30000 PNG. The huge PNG is now probed five times; before, one probe was timed and recorded as "best of 5". The values are recorded before the asserts, so a failure still leaves them in `decoder-suite-records.txt`. CI runs this case in the `decoder-suite` job on every push.
- `src/decoder/tests/probe_bounds.rs`: the module comment now names the real cause of the host-process overhead and points to IT-10-4. Its asserts are unchanged.

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3858_decoder_probe | Done | 2 test files | IT-10-4 1/1 on the container; `probe_bounds` 2/2 on the host | None |

## Code Review Verdict: PASS

Review of the diff (test-only, two files): AC-1 traced to the new asserts; the median of five is the per-photo measure (one outlier on a CI runner does not fail it, a decode would). One Low style finding fixed in place: `Peer` and `Job` are imported instead of written as full `crate::` paths. No security, performance or architecture findings.

## Test Suite

- `DECODER_IMAGE=sunnycloud-decoder:e2e cargo test -p decoder-runner --test container -- --ignored it_10_4_probe`: 1 passed (medians 25 to 27 ms, huge PNG 26 ms).
- `cargo test -p decoder-runner --test probe_bounds`: 2 passed.
- CI-parity: PASS (`rustfmt --check --edition 2024` on both files; `cargo clippy -p decoder-runner --all-targets --locked -- -D warnings`).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | The guard's process-group checks read the whole `/proc` table. That is cheap in the decoder's own pid namespace but costs about 20 ms per read on a host with a thousand processes, five to seven reads per probe | AZ-3858 AC-1, 10 section 3 (runner is the subreaper in its container) | No change: the decoder is deployed only in its own container. A run outside a container (developer host) is slower but still correct | clear |
| 2 | No `moov` within the first 64 top-level boxes | AZ-3858 flagged concern (Low, open) | The code answers `moov_first = false`, the safe value for the remux rule; stays with the E-19 owner | clear |

## Commit

`[AZ-3858] Probe latency bounds asserted on the decoder container` — Loop: 1

## Next Batch

AZ-3847 (media integration suite), AZ-3878 (worker integration suite): AZ-3858 was their last open dependency.
