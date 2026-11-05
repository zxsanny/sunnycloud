# CommitService commit_upload: blocklist check, hash roll-up, sniffing, BlobCommitEffect

**Task**: AZ-3738_transfer_commit_upload
**Name**: Transfer commit_upload
**Description**: Commit one large upload: check the blocklist against the received hashes, derive the file hash without reading file bytes, sniff the type from the head, choose the tree target, and call `TreeCommit::publish` with `BlobCommitEffect` and the injected effects
**Complexity**: 5 points
**Dependencies**: AZ-3731_transfer_spike_resend_stale_base, AZ-3735_transfer_put_block, AZ-3736_transfer_resume_abort, AZ-3709_tree_publish_core, AZ-3710_tree_conflict_rules, AZ-3711_tree_quota_gc_deltas, AZ-3712_tree_blob_ownership_check, AZ-3716_tree_revision_lookups, AZ-3766_sc_ids, AZ-3728_blob_reader, AZ-3729_blob_registry_gc_queue
**Component**: 03_transfer
**Tracker**: AZ-3738
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — port signatures and DTOs produced by E-00/c04.
- `_docs/02_document/contracts/03_transfer/commit_service.md` — produced by this task (E-14/c09 extends it).
- `_docs/02_document/contracts/02_tree/tree_commit.md` — `publish` and the `CommitEffect` protocol.
- `_docs/02_document/contracts/02_tree/tree_reader.md` — `get_revision` for target choice.

## Problem

Finishing a 50 GiB upload must take constant time (AC-4): no file bytes may be read beyond a small head. The type must come from the bytes, never from the name or the client (AC-47 depends on it). 03 must not open the commit transaction (ADR-015): it hands `publish` the entry and an effect that flips the blob to committed and deletes the session row inside 02's transaction, so a reaped session fails only its own entry and gives back its quota release.

## Outcome

- `commit_upload` returns `Committed{node_id, revision_id, conflict_copy?}` or a typed error, in time independent of file size.
- The revision's content hash equals the BLAKE3 roll-up of the block hashes; the media type is sniffed from at most 64 KiB of the head.
- `BlobCommitEffect` locks the session row, flips the blob and deletes the row inside the commit transaction; zero rows fail the entry with `UploadNotFound`.

## Scope

### Included
- Outside any transaction: compare `blocklist` with the progress or checkpointed `block_hashes` (`NeedBlocks(list)` for missing bits, `BlocklistMismatch` for a wrong hash or count, `UploadNotFound`), whole-file hash from the 32-byte hashes (helper 04 roll-up), `BlobReader::read_head` (<= 64 KiB) and byte sniffing (unknown = no type).
- Target choice: with `base_revision`, `Target::Existing` on the node of that revision (`TreeReader::get_revision`); without it, `Target::New{parent_id, name}`; `NewRevision` carries `reserved_bytes` from the session row, `client_mtime`, `client_meta`, `import_hint`, `vault = false`.
- `BlobCommitEffect.on_entry`: `SELECT … FOR UPDATE` on the session row, `BlobRegistry::mark_committed_in_tx`, delete the row; zero rows is `EffectError::Entry(UploadNotFound)`.
- One `publish` call with `CommitActor{account_id, device_label}` and the effects list (own effect plus the injected ones).
- Mapping of outcomes and entry errors to `Committed` and `InvalidName`, `ParentGone`, `ParentNotFound`, `NotAFolder`, `AccountNotActive`, per the commit contract; metrics `commits_total{kind = block_list}`, `commit_duration_seconds`.

### Excluded
- Batch commits and receipts (E-14/c09); the reaper race suite (E-14/c11); crash and statement-log cases (E-14/c12).

## Acceptance Criteria

**AC-1: Hash roll-up without reading the file (AC-4, AC-7)**
Given a fully uploaded file
When it is committed
Then the revision's content hash equals an independently computed BLAKE3 of the file, and the reader decorator counts exactly 1 `read_head` call of at most 64 KiB and 0 full reads

**AC-2: Type from bytes only (AC-47)**
Given JPEG bytes named `photo.png` with client type `text/html`, HTML bytes named `x.jpg`, and random bytes
When each is committed
Then their media types are `image/jpeg`, `text/html` and none

**AC-3: Blocklist checks**
Given an upload missing block 2, a blocklist with one wrong hash, a blocklist of 3 hashes for 5 blocks, and an unknown upload id
When each is committed
Then they return `NeedBlocks([2])`, `BlocklistMismatch`, `BlocklistMismatch` and `UploadNotFound`, and on success the session row is deleted

**AC-4: Target choice and conflict outcomes (AC-8)**
Given a file at its current base, the same file with a stale base, and a new file whose name is taken
When each is committed by device `phone-2`
Then the first keeps the node id with a new revision, the second returns `Committed` with the conflict copy holding the old content and no `EditedSince`, and the third returns `Committed` with the server's `name (conflict copy … phone-2)` name and no `name_conflict`, with fields as the commit contract records from E-14/c01

**AC-5: Session removed between validation and publish (AC-68, AC-74)**
Given validation has passed and the session row is then removed by `abort_upload` before `publish` (decorator pause)
When the commit continues
Then it returns `UploadNotFound`, no revision exists, usage is unchanged, and the reservation was released exactly once (by the remover)

**AC-6: Interrupted upload resumes and commits (AC-3, AC-4)**
Given a 100 MiB upload started with `client_key` k and 40 MiB sent before the connections drop
When start is called again with k, the missing blocks are sent and the upload is committed
Then the same `upload_id` and only the missing blocks are returned, the committed file's hash equals the source, and the resend is at most 10 MiB or the bound E-14/c01 recorded

## Non-Functional Requirements

**Performance**
- Commit p95 at most 100 ms (fail 2 s) and within 20% between 1 MiB and 8 GiB (fail 50%); measured by PT-03-2 in E-14/c14.

**Reliability**
- Sniffing and roll-up run before the transaction; the effect does no I/O.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Sniffer on DS-17 positives and negatives | Types from bytes; unknown is none |
| AC-3 | Blocklist comparison | Missing, wrong and count cases distinguished |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Uploaded file | Commit with reader decorator (IT-03-6) | Hash equal; 1 head read | FT-P-03 |
| AC-2 | JPEG/HTML/random bytes | Commit (IT-03-6) | Sniffed types | FT-N-11, FT-P-39 |
| AC-3 | Four bad inputs | Commit (IT-03-6) | `NeedBlocks`, `BlocklistMismatch` x2, `UploadNotFound` | FT-N-01 |
| AC-4 | Current, stale, taken name | Commit (IT-03-7) | Same id; conflict copy; server name | FT-P-10 |
| AC-5 | Row removed mid-commit | Commit (IT-03-9 case a) | `UploadNotFound`; released once | NFT-RES-34 |
| AC-6 | 100 MiB, 40 MiB sent, drop | Resume with same key, commit (AT-03-1) | Same id; hash equal; resend in bound | FT-P-01 |

## Constraints

- Real 02 `TreeCommit`/`TreeReader`, 04 reader and registry on a temp directory, 07a and 05 on one PostgreSQL; only decorators delegating to real ports.
- The media effect from 08 is injected by the composition root; its real run is E-14/c15.

## Risks & Mitigation

**Risk 1: Effect contract drift with 02 (E-14 risk 4)**
- *Risk*: the effect reads beyond its rows or runs after the stamp.
- *Mitigation*: tests use 02's real `publish`; the statement-log check is E-14/c12.

## Runtime Completeness

- **Named capability**: media-type sniffing from file bytes at commit (03 §3, AC-47).
- **Production code that must exist**: a sniffer over the `read_head` bytes that returns the real type for the AC-47 allowlist and for the types 08 plans jobs for.
- **Allowed external stubs**: none; tests use DS-17 and generated media.
- **Unacceptable substitutes**: deciding by file extension or client-declared type, or returning "unknown" for every file.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-03-3: commit response for stale-base and taken-name commits, and whether `EditedSince` can occur (E-14/c01) | Owner | open | Medium |
| Both 03 (upload sniffing) and 08 (worker output check) sniff bytes; no shared helper is planned, so two sniffers may disagree | E-14, E-18 owners | open | Low |
| `tests/spec-gaps.md` still lists G-10 (inline by MIME, sniffing or name) as an owner item, while 03 §3 says sniffing resolves it | Owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/03_transfer/commit_service.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
