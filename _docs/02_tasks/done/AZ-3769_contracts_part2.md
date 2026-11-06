# contracts part 2: 02 tree, 03 transfer, 05 sync_feed ports and the commit-effect protocol

**Task**: AZ-3769_contracts_part2
**Name**: contracts part 2 (tree, transfer, sync feed, CommitEffect)
**Description**: Declare the commit-path and change-feed ports of 02, 03 and 05, the `CommitEffect` protocol run inside 02's transaction, and the blob data-plane value types that 03's signatures need, signatures only
**Complexity**: 5 points
**Dependencies**: AZ-3768_contracts_part1
**Component**: contracts (server ports)
**Tracker**: AZ-3769
**Epic**: AZ-3621 (E-00)
**Ref**: E-00/c04

## Document Dependencies

- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — conventions, `Principal`, `PrincipalRevalidator`, `AccountLifecycleHook`, produced by E-00/c03.
- `_docs/02_document/contracts/shared_ids_hashing/sc_ids.md` — `Hash32`, `FileHash`, produced by E-04/c01.
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — `Timestamp`, produced by E-03/c01.
- `_docs/02_document/contracts/shared_error/sc_error.md` — `ErrorCode` in `EffectError`, produced by E-02/c01.

## Problem

The first vertical slice (upload, commit, feed, download; the answer to R15) runs through 02's single commit transaction, with 03 and 08 adding effects and 05 stamping the feed last. These are the most coupled ports in the system (AC-68, AC-69, AC-74): if their shapes are not fixed before E-12, E-13 and E-14 start, the three epics cannot build against each other's doubles.

## Outcome

- Every port of 02, 03 and 05 and the `CommitEffect` protocol exist as object-safe traits with the contract's DTOs and error variants.
- The four blob data-plane types (`BlobRef`, `ByteStream`, `PlaintextStream`, `ByteRange`) exist once, so part 2 compiles before part 3; 04 keeps ownership of later changes.
- The listing test covers parts 1 and 2.

## Scope

### Included
- 02: `TreeService`, `TreeReader`, `TreeCommit`, `TreeRestore`, `LiveUsageSource`, `TreeMaintenance`, and `CommitActor`, `PublishEntry`, `Target`, `NewRevision`, `EntryResult`, `PublishOutcome`, `LiveFile`, node and revision views.
- The commit-effect protocol: `CommitEffect` (`on_begin`, `on_entry`, `on_batch`), `Begin`, `EffectError` (`Entry`, `Batch`), `PublishedEntry`, `EntryOutcome`.
- 03: `UploadService`, `CommitService`, `DownloadService`, `ReservationSource` and their DTOs.
- 05: `FeedStamp`, `FeedNotifier`, `FeedHorizon`, `ChangeFeed`, `EpochAdmin`, `Cursor`, `ChangePage`, `ChangeItem`, `MarkerStatus`.
- The 04-owned value types `BlobRef`, `ByteStream`, `PlaintextStream`, `ByteRange`.
- Listing test extended to part 2; one recording double per trait; exhaustive-match tests on `PublishOutcome` (7 variants) and `EffectError` (2 variants).

### Excluded
- Any behaviour (E-12, E-13, E-14); 04, 06, 08, 20 ports (E-00/c05).

## Acceptance Criteria

**AC-1: Every part-2 port declared (E-00 AC 3, part 2)**
Given the contract's lists for parts 1 and 2
When the listing test runs
Then 0 names are missing

**AC-2: Effects can be supplied per call**
Given a test `CommitEffect` that returns `Begin::Replay`, `EffectError::Entry(code)` and `EffectError::Batch(code)` from its three methods
When it is passed to the `TreeCommit::publish` signature of a recording double as `Arc<dyn CommitEffect>`
Then the test compiles and the double records the effect list

**AC-3: Outcome sets are closed**
Given exhaustive matches on `PublishOutcome` and `EffectError` without a wildcard arm
When the tests compile
Then `PublishOutcome` has exactly `Created`, `Updated`, `Replaced`, `ConflictCopied`, `CreatedAsCopy`, `Revived`, `NoChange`, and `EffectError` exactly `Entry` and `Batch`

**AC-4: Folder and gone are distinct**
Given `TreeReader::resolve_live_file`
When its result type is matched
Then a folder is a found `LiveFile` with `kind = Folder` and empty revision and blob id, and a gone node is a separate `Gone` value, never an error

**AC-5: Transactions are joined only where listed**
Given the part-2 signatures
When the compile tests run
Then `FeedStamp::stamp`, `FeedHorizon::raise_purge_horizon` and the three effect methods take `&mut DbTx`, and no other part-2 method does

**AC-6: One BlobRef**
Given the crate
When it is searched for type definitions named `BlobRef`
Then exactly one exists, with the four fields of the contract

**AC-7: Signatures only**
Given the crate
When it is searched for `todo!(`, `unimplemented!(`, HTTP and sqlx types
Then 0 hits, and the crate-graph test passes

## Non-Functional Requirements

**Compatibility**
- Builds with the workspace MSRV and `--locked`; clippy `-D warnings` clean.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Listing test, parts 1-2 | 0 missing |
| AC-2 | Test effect passed to a recording `publish` | Compiles; recorded |
| AC-3 | Exhaustive matches | 7 and 2 variants |
| AC-5 | Compile tests of `&mut DbTx` placement | As listed |
| AC-6 | Search for `BlobRef` definitions | Exactly one |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| — | None: no runtime behaviour; exercised by E-12..E-14 and the pipeline tasks (large and small upload, change feed, download) | — | — | — |

## Constraints

- ADR-015: 02 owns the commit transaction; effects do no I/O and add no statement after the stamp (contract invariants).
- Part-1 conventions apply.

## Risks & Mitigation

**Risk 1: The 03/04 type cycle between parts**
- *Risk*: 03's signatures need 04's `BlobRef` and stream types (part 3), while 08's `DerivedOpener` in part 3 needs 02's `LiveFile` (part 2).
- *Mitigation*: the four 04 value types are declared here; part 3 uses them. Ownership of later changes stays with 04 (module-layout Rule 6).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 03 puts `media_type` inside `BlobRef`; 04 (owner) excludes it. The contract follows 04 and passes `media_type` separately to `open_blob_content`. | E-10 / E-14 owners | open | Medium |
| 08 describes `on_batch(tx, &[PublishedEntry])` and a `CommitContext` with `account_id`; 02 defines `on_batch(tx, &[EntryOutcome])` and `PublishedEntry` without `account_id`, which 08's job key needs. | E-13 owner with E-18 | open | Medium |
| `StoredResponse` (replayed batch response) has no specified type; it must be 03's serialised `BatchResult`. | E-14 owner | open | Low |
| `ChangeFeed` "takes a shutdown signal" (05) without a method; the contract leaves it to construction by 12. | E-12 owner | open | Low |
| `NodeView`, `RevisionView`, `ListingPage` field lists are derived from data_model §5 and 05's `ChangeItem`; 02 lists none. | E-13 owner | open | Low |
| `LiveFile.blob_ref` is a blob id (02), although AZ-3652's DTO table uses the same word as the `BlobRef` struct. | E-13 owner | open | Low |
| The 04 value types are declared in this part, not in part 3, so the parts compile in order; this moves work between two rows of the same epic. | E-00 owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
