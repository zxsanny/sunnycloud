# Rewrap, epoch marker, missing-blob inventory, health probe and doctor checks

**Task**: AZ-3746_blob_maintenance_ports
**Name**: Blob maintenance ports
**Description**: Key rotation by rewrap without touching files, the atomic `.epoch` marker, the missing-blob inventory, the blob health probe and contributor, and the blobstore doctor checks
**Complexity**: 5 points
**Dependencies**: AZ-3725_master_keyring, AZ-3726_blob_writer, AZ-3728_blob_reader, AZ-3729_blob_registry_gc_queue, AZ-3743_orphan_sweeper, AZ-3768_contracts_part1
**Component**: 04 blobstore
**Tracker**: AZ-3746
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c13

## Problem

The operator rotates the master key without rewriting blobs (AC-43). After a restore, 05 and 12 need the `.epoch` marker to notice a missing epoch bump (R02, AC-65), `doctor` must list blobs whose files are gone (AC-73), and health and readiness need the state of the blob volume. These are small ports, each read by another component.

## Outcome

- `MasterKeyring::rewrap_batch(from_id, to_id, n)` re-wraps `wrapped_key` for up to n rows in short transactions and never rewrites files; it completes the `MasterKeyring` port of E-10/c05.
- `EpochMarker::write(epoch)` and `read()` are the only access to `<blob_root>/.epoch`; the write is atomic.
- `BlobInventory::find_missing(batch)` lists committed blobs whose file is absent, with a throttled file check.
- `BlobHealth::probe()` returns `{writable, free_bytes, below_floor, unavailable_count}`; the blobstore `HealthContributor` reports from it.
- Blobstore `DoctorCheck`s: master key and key check; volume writable, filesystem type and free space under the floor; retention against `--backup-interval`; missing blobs.

## Scope

### Included
- The five items above and their INFO logs (rotation batch).

### Excluded
- The epoch value and the missing-bump check (05, E-12/c07); `serve` refusal, `admin rewrap` and `doctor` aggregation (E-22/c05, c11, c14); revision paths for the missing list (02 `describe_blobs`); metrics (E-10/c15).

## Acceptance Criteria

**AC-1: Rotation by rewrap**
Given 450 blobs wrapped under key 1 and a ring where key 2 is current
When `rewrap_batch(1, 2, 100)` is repeated until it reports 0 rows, with one run interrupted after its second batch and resumed
Then every `wrapped_key` uses id 2, the SHA-256 of every blob file is unchanged, reads work during and after, new blobs use id 2, and after key 1 is removed from the ring `load_and_verify` passes and every blob is readable

**AC-2: Epoch marker is atomic**
Given a fresh volume
When `read()` runs, then `write(7)` and `read()`, and then a process is killed during a `write`
Then the reads return `Absent`, then 7, and after the kill either the old or the new value, never a partial one

**AC-3: Health probe**
Given a volume with 5 blobs marked unavailable, a read-only blob root, and a 64 MiB volume filled past the floor
When `probe` runs
Then `unavailable_count` equals the count of rows with `unavailable_at` set (5), `writable` is false for the read-only root, and `below_floor` is true with `free_bytes` within 1 MiB of the filesystem's figure; the contributor reports `Degraded` while `unavailable_count > 0`

**AC-4: Missing-blob inventory**
Given 50 committed blobs with 7 files deleted, and 2 `uploading` rows without a file
When `find_missing(batch = 20)` is called until it has covered every committed row
Then exactly the 7 ids are returned and no `uploading` row, and the missing-blob doctor check lists the same 7 ids as `Fail` findings

**AC-5: Restore drill**
Given 100 blobs with 5 files removed to mimic retention-expired blobs
When the orphan sweep runs in `report` mode and then `find_missing` runs
Then the sweep deletes nothing, `find_missing` returns the 5 ids, and reads of those blobs return `BlobMissing`

**AC-6: Key doctor check**
Given a stored key check and a different master key
When the key doctor check runs
Then it reports a `Fail` finding with no key bytes in the message

**AC-73: Missing blobs are typed findings, never errors (component part, R6)**
Given a restored database whose committed rows reference 3 blob files that are absent
When `find_missing` covers all rows, the reader opens those blobs, a scrub run passes them and the GC sweeps one of them
Then exactly those 3 ids are reported, opens return `BlobMissing` (callers answer 404 `content_unavailable`, never 5xx), the scrub counts them as missing and the GC logs the missing file and removes its rows without failing; `doctor` paths, exit code and user listings are checked end to end by E-22/p13

## Non-Functional Requirements

**Performance**
- `find_missing` checks files at a throttled rate; rewrap runs in short transactions so reads continue.

**Reliability**
- Rewrap is resumable and idempotent in effect (a re-run finds no rows left under the old id).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Rewrap idempotence on an already rewrapped row | Row unchanged; counted as done |
| AC-2 | Marker parse of decimal text; partial file | Partial content never returned as a value |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 450 blobs, ring 1 and 2 | IT-04-13 rewrap part; AT-04-1 | Ids all 2; files byte-identical; key 1 removable | FT-P-41 |
| AC-2 | Fresh volume | IT-04-16 marker part | `Absent`, 7, old or new after kill | NFT-RES-07 |
| AC-3 | Unavailable rows; read-only root; full volume | IT-04-16 probe part; IT-04-7 probe part | Fields as specified; `Degraded` | NFT-RES-LIM-01 |
| AC-4 | 50 committed, 7 missing, 2 uploading | IT-04-15 | 7 ids; doctor `Fail` for the same ids | |
| AC-5 | 100 blobs, 5 missing | AT-04-3 | 0 deletions; 5 ids; `BlobMissing` | NFT-RES-10 |
| AC-6 | Other key | Key doctor check | `Fail`, no key bytes | FT-N-33 |
| AC-73 | Restored DB, 3 missing files | Inventory, reader, scrub, GC | Typed findings only | NFT-RES-37 |

## Constraints

- No transaction is held across file checks, marker writes or directory reads; rewrap uses short transactions in system scope.
- Findings, logs and errors carry ids only; never file names or key material.
- The blob volume is mounted only in the API container; the master key never lies on it.

## Risks & Mitigation

**Risk 1: Marker rolled back with the volume**
- *Risk*: database and volume restored to the same point; the marker matches and does not reveal the restore.
- *Mitigation*: documented in the description; 05's `S > seq` check protects devices that were ahead.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Finding levels are stated only for missing blobs and key-check failure (`Fail`); the level for ext4, below-floor, read-only volume and the retention-against-`--backup-interval` rule (R33: retention minus GC interval must exceed the copy time) are not stated | Epic owner / E-22 | open | Medium |
| `find_missing(batch)` has no cursor parameter; how repeated calls page through all committed rows is not stated | Epic owner | open | Low |
| The health contributor's level for `writable = false` or `below_floor` is not stated (only `unavailable_count > 0` gives `Degraded`) | Epic owner / E-17 | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/04_blobstore/epoch-marker-and-health.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
