# Batch Report

**Batch**: 40
**Tasks**: AZ-3722_group_commit_fs_spike (measurement spike; no code in the repository)
**Date**: 2026-11-11

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3722_group_commit_fs_spike | Done: both filesystems measured, conclusion recorded. The owner ran the one privileged step (two 20 GiB loop-mounted images on the /develop NVMe) | `04_blobstore/description.md` (Risks row "Group commit stalls on a slow disk") | measurement only: 4 probe runs | Items 1-3 |

## Measurements (informational, non-reference host)

Probe: 400 writers, 2 MiB blocks, leader/follower group `fdatasync` per dirty file, zero coalesce delay, equal 20 GiB images (fallocate) on one NVMe (Viper VP4300L, Ryzen 9 3900XT, kernel 7.0.0-42), one run per row, XFS run first.

| Filesystem | Load | Ack wait p50 / p95 / p99 / max (ms) | Flushes per s | MB/s |
|---|---|---|---|---|
| XFS | 112 MB/s offered, 180 s | 5.1 / 13.1 / 23.4 / 214 | 49.3 | 112 delivered |
| ext4 | 112 MB/s offered, 180 s | 5.7 / 14.6 / 23.2 / 126 | 49.1 | 112 delivered |
| XFS | saturated, 60 s | 1417 / 2518 / 2731 / 2904 | 1.2 | 508 |
| ext4 | saturated, 60 s | 3670 / 4375 / 4417 / 4427 | 0.6 | 237 |

Both meet PT-04-3 at the offered load (p95 <= 100 ms, 250 ms failure, >= 112 MB/s). The saturated rows measure queueing behind about 800 MiB of outstanding blocks, not a stall.

## Conclusion recorded

XFS stays the default; `blob_sync_coalesce_delay` stays 0 ms; ext4 meets the targets at design load but has about half the saturation headroom (237 vs 508 MB/s), so the operator guide should say: XFS recommended, ext4 accepted with the `block_fsync_duration_seconds` alert. Gate S14: PASS informational; `NOT-RUN(HW-REF)` for the gate itself until REF-SRV exists.

## Code Review Verdict: PASS

Documents only. The probe lives outside the repository (AC-4); the Jira issue is `In Testing`.

## Test Suite

Not run: no code changed (last full run in batch 37: 2762 passed, 0 failed, 9 ignored).

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | One run per configuration, XFS before ext4, loop devices over a file on ext4 (an extra layer the real volume does not have); the gap between the two filesystems is large enough (2.1x saturated) to survive that, but the exact figures need a repeat on REF-SRV | AC-2 | Labelled informational; repeat in E-10/c14 | unclear |
| 2 | The probe's printed MB/s divides by an elapsed time that includes the tail wait for the last writers (107 instead of 112 delivered in the paced rows) | AC-1 | Delivered rate recomputed from the write count over the 180 s window | clear |
| 3 | The two images are still mounted under `/develop/sc-fs-spike/` (40 GB) | | The owner removes them with `unmount.sh` (needs sudo) | unclear |

## Commit

`[AZ-3722] Group commit filesystem spike: XFS stays the default` — Loop: 1

## Next Batch

Ready now: AZ-3893 (Windows only) and 25 decision spikes (AZ-3787, 3799, 3849-3851, 3867, 3881-3887, 3913, 3921, 3930, 3933, 3934, 3973, 3985-3988, 4004, 4005). 245 tasks left.
