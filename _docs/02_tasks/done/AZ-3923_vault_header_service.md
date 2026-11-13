# VaultService: create, read and replace the vault header with a SHA-256 ETag and If-Match

**Task**: AZ-3923_vault_header_service
**Name**: Vault header service
**Description**: Implement `create_vault`, `get_header` and `replace_header` of the `VaultService` port: the opaque header is stored and returned byte for byte, its ETag is the SHA-256 of the stored bytes, a replace succeeds only with the current ETag, and the header cap and the vault-root rules hold
**Complexity**: 3 points
**Dependencies**: AZ-3913_vault_decisions_spike, AZ-3922_vaults_migration_config, AZ-3770_contracts_part3, AZ-3707_tree_reader, AZ-3766_sc_ids, AZ-3764_sc_error, AZ-3782_sc_db_access
**Component**: 20 vault_service [v1.2]
**Tracker**: AZ-3923
**Epic**: AZ-3648 (E-30)
**Ref**: E-30/c03

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — `VaultService` port (E-00/c05; v1.2 proposal, frozen to 1.x by this task).
- `_docs/02_document/contracts/02_tree/tree_reader.md` — reading the root node (E-13/c03).
- `_docs/02_document/contracts/shared_ids_hashing/sc_ids.md` — SHA-256 helper (E-04/c01).
- `_docs/02_document/contracts/shared_db/sc_db_access.md` — account scope, short transactions (E-06/c01a).
- `_docs/02_document/contracts/shared_error/sc_error.md` — typed business errors (E-02/c01).

## Problem

Two clients of one vault (a passphrase change on one device, a recovery on another) both rewrite the vault header. Without a precondition one would silently overwrite the other. The server must keep the header as opaque bytes (it never parses it), return it unchanged so the client's authentication check works (AC-61), and let exactly one of several concurrent replacements win. A vault may only be created on a live folder of the caller.

## Outcome

- A header stored by `create_vault` or `replace_header` is returned by `get_header` byte for byte, with an ETag equal to the SHA-256 of the stored bytes.
- `replace_header` with a stale ETag changes nothing; under concurrency exactly one replacement wins.
- The `VaultService` rows of `contracts` part 3 are frozen to a 1.x version with the E-30/c01 answers.

## Scope

### Included
- `create_vault(account, header, root_node_id) -> (vault_id, ETag)`: the root must be a live folder of the caller (`TreeReader`); header length at most `max_vault_header_bytes`; one vault per root; `VaultExists` only if E-30/c01 decided one vault per account.
- `get_header(account, vault_id) -> (header, ETag)` and the root node id the GET route returns.
- `replace_header(account, vault_id, header, if_match) -> ETag`: the stored header changes only when `if_match` equals the ETag of the stored bytes; otherwise `PreconditionFailed` and the stored header is unchanged.
- ETag computed with the shared hashing helper over the stored bytes.
- Every query scoped by account; primary-key lookups only; short transactions.
- Freezing the `VaultService` rows of `contracts` part 3 (with the E-00 contract owner), including any variant this task needs (see Flagged concerns).

### Excluded
- The HTTP mapping: routes, the `ETag` and `If-Match` headers, 412 and 428 (E-30/c06).
- `report_unlock_failure` (E-30/c04) and the erase hook (E-30/c05).
- Any parsing of the header, the generation counter and rollback detection (E-29, clients).

## Acceptance Criteria

**AC-1: Byte-identical round trip (AC-61, IT-20-1)**
Given a live folder of account A
When vaults are created with headers of 1 byte, 512 bytes, a block holding all 256 byte values, and exactly `max_vault_header_bytes`, and each is read back, also after a service restart
Then every read returns the same bytes and the ETag equals the SHA-256 of those bytes on every read

**AC-2: Header cap**
Given `max_vault_header_bytes` = N
When a header of N + 1 bytes is sent to `create_vault` and to `replace_header`
Then both return `HeaderTooLarge` and nothing is stored or changed

**AC-3: Root must be a live folder of the caller (IT-20-7)**
Given a folder of account B, a random UUID, a tombstoned folder of A, a file of A and a live folder of A
When A calls `create_vault` with each as root
Then the first three return `NotFound`, the file is refused with nothing stored, and the live folder succeeds; a second `create_vault` on the same root is refused with nothing stored

**AC-4: Precondition on replace (AC-61, IT-20-2)**
Given a vault with header H0 and ETag E0
When `replace_header` is called with a stale ETag, then with E0 and header H1
Then the stale call returns `PreconditionFailed` with the stored header byte-equal to H0, and the second returns a new ETag equal to the SHA-256 of H1

**AC-5: Concurrent replacement**
Given a vault with ETag E1
When 50 concurrent `replace_header` calls each send their own header with `if_match` E1
Then exactly 1 succeeds, 49 return `PreconditionFailed`, the stored header equals the winner's bytes, and no call fails with an unexpected error

**AC-6: Account scope (AC-59)**
Given a vault of account A
When account B calls `get_header` or `replace_header` with A's vault id
Then both return `NotFound`, the same answer as for an unknown id, and A's header is unchanged

**AC-7: Vaults per account**
Given the E-30/c01 decision
When A creates a second vault on another live folder
Then with "one per account" the call returns `VaultExists` and nothing is stored; with "many" it succeeds

**AC-8: Purging a tombstoned vault root removes the vault**
Given a vault whose root folder was deleted through 02 and whose tombstone is past the retention window
When 02's tombstone purge runs
Then the purge succeeds, the root and the vault row are gone, and the account can create a new vault

## Non-Functional Requirements

**Performance**
- Primary-key lookups only; no scan (epic NFR "Query shape").

**Reliability**
- Coverage at least 90% on the replace precondition and account-scoping paths (`quality-thresholds.md`).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | ETag of known byte strings (1 byte, the 256-value block) | Equals the SHA-256 of the bytes |
| AC-2 | Cap check at N and N + 1 | N accepted, N + 1 `HeaderTooLarge` |
| AC-4 | Precondition compare with equal, different and malformed ETags | Only the equal ETag passes |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Real PostgreSQL 18 (testcontainers), live folder | Create and read four header sizes, restart, read again | Bytes and ETag equal on every read | IT-20-1, AC-61 |
| AC-2 | Cap N from config | Create and replace with N + 1 bytes | `HeaderTooLarge`; nothing stored | IT-20-1 |
| AC-3 | Foreign, unknown, tombstoned, file and live roots | `create_vault` with each; second vault on one root | `NotFound` x3; file refused; live ok; second refused | IT-20-7, AC-59 |
| AC-4 | Vault with E0 | Stale replace; replace with E0 | `PreconditionFailed`, H0 kept; new ETag = SHA-256(H1) | IT-20-2 |
| AC-5 | Vault with E1 | 50 concurrent replaces | 1 success, 49 `PreconditionFailed`, winner stored | IT-20-2 |
| AC-6 | Accounts A and B | B reads and replaces A's vault | `NotFound`; header unchanged | ST-20-1, AC-59 |
| AC-7 | c01 decision | Second vault on another root | `VaultExists` or success per decision | IT-20-7 |

## Constraints

- 20 never imports `vault-core` and never parses the header (ADR-015, 20 §3).
- Port-only access to 02 through `TreeReader` (ADR-015); no call from 20 to 03 or 06.

## Risks & Mitigation

**Risk 1: Race decided outside the database**
- *Risk*: a check-then-write in two statements lets two replacements both pass.
- *Mitigation*: AC-5 runs against real PostgreSQL with 50 concurrent calls; a lost update fails it.

## Runtime Completeness

- Named capability: optimistic concurrency on the header with a SHA-256 ETag.
- Production code that must exist: the ETag computed over the stored bytes with the shared hashing helper, and a replace whose success depends on the stored bytes at write time.
- Allowed external stubs: none for AC-1..AC-7 (real PostgreSQL); `TreeReader` may be the real 02 crate or its contract double only in unit tests.
- Unacceptable substitutes: a version counter in place of the hash, a hash of the request copy, an in-memory store in the concurrency test.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The error variant for a file as root and for a second vault on the same root is not defined (port lists only `VaultExists` "if decided", `HeaderTooLarge`, `NotFound`); IT-20-7 says "4xx per the 11 mapping" | E-30 / E-00 contract owner | decided 2026-11-13 (owner): a file root and the account's own root folder (even when empty) answer `NotFound`; a second vault (same or other root) or a non-empty root answers `VaultExists` | Medium |
| `get_header` in `contracts` part 3 returns `(header, ETag)` but the GET route also returns the root node id (20 §4); part 3 needs the field when frozen | E-00 contract owner / E-30 | decided 2026-11-13: `VaultHeader.root_node_id` added (part 3 1.3.0) | Low |
| A tombstoned vault root (deleted through 02 `delete_node`) makes the 90-day tombstone purge (AZ-3714) fail on `vaults_root_node_id_account_id_fkey` on every run, blocking that batch. Before the first vault is created, either 02 refuses deleting a vault root (via the planned `nodes.vault_id`) or the purge skips nodes referenced by `vaults`. Found 2026-11-13 (batch 52) | E-30 / E-13 | decided 2026-11-13 (owner): the vault is removed with its purged root (migration `20261113120000_cascade_vault_on_root_purge`, `ON DELETE CASCADE`); AC-8 | High |
