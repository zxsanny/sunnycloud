# Contract: epoch-marker-and-health

**Component**: 04_blobstore
**Producer task**: E-10/c13 — `_docs/02_tasks/todo/B02-13_blob_maintenance_ports.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-12/c06 and E-12/c07 (`EpochMarker`), E-22 operator CLI (`serve` epoch check, doctor registry, `admin rewrap`), E-17/c07 (health aggregation through `HealthContributor`), E-21/c16 (`/readyz`, `/metrics` through 12)
**Version**: 1.1.0
**Status**: draft (frozen when E-10/c13 is accepted)
**Last Updated**: 2026-11-08

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
- `probe` reports `unavailable_count` equal to the number of `blobs` rows with `unavailable_at` set, `free_bytes` within 1 MiB of the filesystem's own figure, `below_floor` when free space is under the floor (the smaller of `blob_free_reserve` and `blob_free_reserve_percent` of the volume), and `writable = false` for a read-only blob root.
- The blobstore `HealthContributor` reports `Degraded` while `unavailable_count > 0` (health is derived from the column, so it survives a restart). Other levels: open point, flagged in E-10/c13.
- `find_missing` returns only committed blobs (never `uploading` ones), checks files at a throttled rate and returns ids only; repeated calls cover all committed rows. The port has no cursor argument, so the implementation keeps the position: each call checks the next `batch` committed rows in id order (at least one) after the previous call's last row and returns the ids among them whose file is absent; after the last row the next call starts again at the first, so `ceil(rows / batch)` calls cover every row. A failed call keeps the position. The concrete type also offers `scan_page(after, batch)`, which is stateless and returns where the next page starts (`None` at the end); the doctor check uses it. A file that cannot be looked up for a reason other than absence (permission, I/O) is an `InventoryError`, never a missing blob. The throttle is one lookup at a time on the blocking pool; no rate value is configured.
- The marker file is `<epoch>\n` (canonical decimal text and a newline), replaced through `.epoch.tmp`, `fsync`, rename and a folder `fsync`. `read` accepts only that form; an empty, cut or non-numeric file is `EpochMarkerError::Io`, never a value. A blob root that does not exist is an error on `read` and `write`, not an absent marker. `Absent` of the task text is `None` of `Option<i64>` in the port.
- `probe` reads `writable` with `access(W_OK)` on the root (a read-only mount and a root without write permission are both `false`), `free_bytes` with the same `statvfs` figure the writer's floor check uses, and `below_floor` as `free_bytes < min(blob_free_reserve, blob_free_reserve_percent of the volume size)`. A root that cannot be queried is `BlobError::Io`.
- The `HealthContributor` is named `blobstore`: `Degraded` while `unavailable_count > 0` (detail: the count), `Ok` otherwise, `Down` when the probe itself fails. Levels for an unwritable root or free space under the floor are not part of it (they are `/readyz`'s through `probe`).
- `MasterKeyring::rewrap_batch(from, to, n)` reads up to `n` rows (any state) wrapped under `from`, re-wraps them in memory, and writes them in a second short transaction guarded by the wrap that was read; it returns the rows handled, so a row another run already moved counts as done and a repeat returns 0. Equal ids, an id the ring does not hold, and a wrap that does not open are `KeyError::Unexpected` and change nothing. It loads and verifies the keyring first if that has not happened. Each batch logs INFO `rewrap batch finished`; the empty final call logs `rewrap finished, no row left under the old key`.
- Doctor findings carry ids, never file names or key material. A missing blob and a key-check failure are `Fail` findings (`doctor` exits 2, G-12-6).
- Checks and levels (from `deployment/observability.md`, `doctor` table): `master_key` (tags `PreFlight`, `Restore`): any refusal that stops a start (key file missing, not 32 bytes, mismatch with the key check, key check absent while blobs exist) is `Fail`; it writes nothing, and with `pre_flight` it opens no database connection. `blob_volume` (tag `PreFlight`): root missing or unreadable, not writable, free space under `blob_free_reserve` are `Fail`; a filesystem that is neither XFS nor ext4 is `Warn`; one finding per aspect. `blob_retention` (no tag): retention 0 is `Warn`; retention shorter than `--backup-interval` is `Warn`. `blob_files_missing` (tag `Restore`): one `Fail` per missing blob id; a scan that cannot finish is one `Fail`. The `Restore` and `PreFlight` tags and the missing-scan batch (constructor argument) are not stated by the task; they follow the `--pre-flight` description of 12 (config, key length, volume).
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
| 1.1.0 | 2026-11-08 | Stated the `find_missing` paging, the marker file form, the probe's sources, the contributor levels, rewrap semantics and the doctor checks with their levels (AZ-3746) | implement |
