# Contract: epoch-marker-and-health

**Component**: 04_blobstore
**Producer task**: E-10/c13 — `_docs/02_tasks/todo/B02-13_blob_maintenance_ports.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-12/c06 and E-12/c07 (`EpochMarker`), E-22 operator CLI (`serve` epoch check, doctor registry, `admin rewrap`), E-17/c07 (health aggregation through `HealthContributor`), E-21/c16 (`/readyz`, `/metrics` through 12)
**Version**: 1.0.0
**Status**: draft (frozen when E-10/c13 is accepted)
**Last Updated**: 2026-11-04

## Purpose

The small ports other components read for restore safety and health: the `.epoch` marker, the blob health probe, the missing-blob inventory and the blobstore doctor checks. Trait signatures: `EpochMarker`, `BlobInventory` in E-00/c05 (contracts part 3); `DoctorCheck`, `HealthContributor` in E-00/c03 (`_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md`). Source: `components/04_blobstore/description.md` §3, §4, §8.

## Shape

| Name | Input -> Output | Errors | Blocking? |
|------|-----------------|--------|-----------|
| `EpochMarker::write` | `epoch: i64` -> `()` | typed write error | async |
| `EpochMarker::read` | `()` -> `Absent \| Present(epoch)` | typed read error | async |
| `BlobHealth::probe` | `()` -> `{writable, free_bytes, below_floor, unavailable_count}` | typed | async |
| `HealthContributor::check` (blobstore) | `()` -> `HealthItem{status, detail}` | none | async |
| `BlobInventory::find_missing` | `batch` -> ids of committed blobs whose file is absent | typed | async |
| `DoctorCheck::run` (blobstore checks) | `DoctorContext` -> `Vec<Finding{level, message, remedy}>` | none | async |

## Invariants

- `EpochMarker` is the only access to the marker on the blob volume. `read` on a fresh volume is `Absent`. `write` is atomic: after a crash during `write`, `read` returns the old or the new value, never a partial one. The value is the installation epoch as decimal text.
- 04 never decides the epoch: 05 calls `write` when it picks the installation epoch and after each epoch bump; 12 compares the database epoch with `read` at `serve` start.
- `probe` reports `unavailable_count` equal to the number of `blobs` rows with `unavailable_at` set, `free_bytes` within 1 MiB of the filesystem's own figure, `below_floor` when free space is under `blob_free_reserve`, and `writable = false` for a read-only blob root.
- The blobstore `HealthContributor` reports `Degraded` while `unavailable_count > 0` (health is derived from the column, so it survives a restart). Other levels: open point, flagged in E-10/c13.
- `find_missing` returns only committed blobs (never `uploading` ones), checks files at a throttled rate and returns ids only; repeated calls cover all committed rows.
- Doctor findings carry ids, never file names or key material. A missing blob and a key-check failure are `Fail` findings (`doctor` exits 2, G-12-6).
- None of these calls holds a database transaction across file I/O.

## Non-Goals

- Not covered: the epoch value rules and the missing-bump doctor check (05, `EpochAdmin`); the doctor exit codes and aggregation (12); mapping a revision to its path for the missing-blob list (02 `TreeReader::describe_blobs`).
- Not covered: `MasterKeyring::rewrap_batch` (called only by 12; behaviour in E-10/c13 and IT-04-13).

## Versioning Rules

- **Breaking changes** (marker format, atomicity, a field of the probe removed, a finding level changed) require a new major version and notice to E-12, E-17 and E-22.
- **Non-breaking additions** (a new probe field, a new doctor check) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| marker-fresh | `read()` on a new volume | `Absent` | IT-04-16 |
| marker-roundtrip | `write(7)`, `read()` | `Present(7)` | IT-04-16 |
| marker-atomic | process killed during `write` | old or new value, never partial | IT-04-16 |
| probe-readonly | read-only blob root | `writable = false` | IT-04-16 |
| probe-unavailable | 5 rows with `unavailable_at` | `unavailable_count = 5`; health `Degraded` | IT-04-11, IT-04-16 |
| missing-7 | 50 committed, 7 files deleted, 2 `uploading` without file | the 7 ids only; doctor lists the same 7 as `Fail` | IT-04-15 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B02) |
