# contracts part 3: 04 blobstore, 06 sharing, 08 media jobs, 20 vault ports

**Task**: AZ-3770_contracts_part3
**Name**: contracts part 3 (blobstore, sharing, media, vault)
**Description**: Declare the remaining server ports of 04, 06, 08 and 20 with their DTOs and typed errors, and complete the listing test so every port of E-00's interface list exists in `contracts`
**Complexity**: 3 points
**Dependencies**: AZ-3769_contracts_part2
**Component**: contracts (server ports)
**Tracker**: AZ-3770
**Epic**: AZ-3621 (E-00)
**Ref**: E-00/c05

## Document Dependencies

- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — conventions, produced by E-00/c03.
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — `BlobRef`, `ByteStream`, `PlaintextStream`, `ByteRange`, `LiveFile`, produced by E-00/c04.
- `_docs/02_document/contracts/shared_ids_hashing/sc_ids.md` — `FileHash`, produced by E-04/c01.

## Problem

The blob store (04) is a dependency of the first vertical slice, share links (06) and media jobs (08) are called by the HTTP layer and the commit path, and the vault service (20, v1.2) must have its boundary before 11 reserves its routes. Until these ports exist, E-00's "every port declared" acceptance cannot pass and E-10, E-16, E-18 cannot code against doubles.

## Outcome

- Every port of 04, 06, 08 and 20 exists as an object-safe trait with the contract's DTOs and error variants.
- The listing test compares the crate with every port name of E-00 § Interface specification and finds 0 missing.

## Scope

### Included
- 04: `BlobWriter`, `BlobReader`, `BlobRegistry`, `BlobCatalog`, `GcQueue`, `Scrubber`, `BlobInventory`, `MasterKeyring`, `EpochMarker`, with `BlobError`, `KeyError`, `BlobPlan`, `BlobHandle`, `DurableToken`, `WrittenBlob`, `GcReason`, `Due`, scrub DTOs.
- 06: `ShareService`, `PublicShareService` (including `authorize_derived`, see Flagged concerns) and their DTOs; one `NotFound` variant for every missing link.
- 08: `ThumbnailStateSource`, `DerivedOpener`, `WorkerAuthenticator`, `WorkerJobs`, `MediaAdmin`, with `ClaimedJob`, `Lane`, `DerivedKind`, outcomes and `WorkerStatus` declared on the API side.
- 20: `VaultService` (v1.2 proposal status).
- The listing test completed against E-00 § Interface specification; one recording double per new trait; single-variant tests for `WorkerAuthenticator` failures and share `NotFound`.

### Excluded
- Any behaviour (E-10, E-16, E-18, E-30); the worker routes and their JSON (E-21); `media-shared`'s worker-side DTOs (E-00/c06).

## Acceptance Criteria

**AC-1: Every port declared (E-00 AC 3)**
Given every port name of E-00 § Interface specification
When the listing test compares them with the public traits of `contracts`
Then 0 names are missing

**AC-2: Ports can be faked**
Given a recording double of each part-3 trait
When the tests compile and run
Then each double is held as `Arc<dyn Port>` and records its calls

**AC-3: Uniform failures where the ACs need them (AC-12, AC-79)**
Given the error types of `WorkerAuthenticator::authenticate` and the `NotFound` result of `PublicShareService`
When exhaustive matches compile
Then the worker authentication failure has exactly one variant and the share lookup has one `NotFound` variant with no reason field

**AC-4: Single writers stay single**
Given the part-3 signatures
When the compile tests run
Then `GcQueue::schedule_in_tx` and the `BlobRegistry` methods take `&mut DbTx`, and no other port can write `blob_gc` (no other method takes a GC reason)

**AC-5: Server-only, signatures only**
Given the crate
When the crate-graph test and the search for `todo!(`, `unimplemented!(`, HTTP and sqlx types run
Then no client crate, `media-shared`, worker or decoder crate depends on `contracts`, and the search finds 0 hits

## Non-Functional Requirements

**Compatibility**
- Builds with the workspace MSRV and `--locked`; clippy `-D warnings` clean.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Listing test against E-00's full list | 0 missing |
| AC-2 | Recording doubles | Compile; record calls |
| AC-3 | Exhaustive matches on the two failure types | One variant each |
| AC-5 | Crate-graph test | Passes |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| — | None: no runtime behaviour; exercised by E-10, E-16, E-18 and the media, share and scrub pipeline tasks | — | — | — |

## Constraints

- ADR-015; ADR-009 (workers reach the queue only through the API; no worker database role); ADR-016 (vault is v1.2).

## Risks & Mitigation

**Risk 1: Worker DTOs exist twice**
- *Risk*: `contracts` (API side) and `media-shared` (worker side) declare the same job shapes; they drift.
- *Mitigation*: identical field and serialised names, recorded in both contracts; E-18/E-20 add a wire test against the real API.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `contracts` may not import `media-shared` (both layer 2) and 11 may not either, so the worker job DTOs are declared twice (here and in `media-shared`). Default: two declarations with a wire test; alternative: allow one of the imports (layering-table change). | Project owner | open | Medium |
| `PublicShareService::authorize_derived` is named by 11 and 08 but missing from 06's method table; its inputs mirror `authorize_content`. | E-16 owner | open | Medium |
| 12 lists `GcSweeper::run_cycle` for `admin gc-run`; E-00 names no `GcSweeper` port. The contract treats it as 04's `Janitor` implementation chosen by name. | E-22 / E-10 owners | open | Low |
| E-10's first child row says "port wiring in `contracts`"; the 04 ports are declared here, so E-10 only implements them. | E-10 owner | open | Low |
| `MediaStats` fields and the scrub status summary shape are not specified. | E-18 / E-10 owners | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
