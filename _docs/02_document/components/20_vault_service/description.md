# 20 Vault Service  [v1.2 — stub-level spec]

> **Release gate**: nothing in this component ships in v1.0 or v1.1 (FT-P-73). It is built for v1.2 together with `19_vault_core` and released only after the external review of the construction passes (AC-62). Routes, port methods and tables below are a proposal for that review, not a frozen contract.

## 1. High-Level Overview

**Purpose**: The server side of the end-to-end encrypted vault. It stores and returns the opaque vault header (with optimistic concurrency) and enforces vault limits. The ciphertext content uses the normal revision and blob path; the per-file wrapped key travels in the revision's own `revisions.client_meta`. It never sees a passphrase, a key or plaintext, and it never links `19_vault_core`.

**Architectural pattern**: Small service behind `contracts` ports with one table (`vaults`). There is no per-file table: a vault file is an ordinary node whose revisions carry the wrapped key in `client_meta`, so the key commits atomically with the revision (no `CommitEffect`, no separate key call).

**Scope in**: the `vaults` table (data model 10.2, reduced), header create/read/replace with `ETag` and `If-Match`, server-side vault size limits (100 MiB per file, from config), vault failure reports for the audit log (best-effort, architecture answer 13), the `erase_batch` lifecycle hook for account deletion.
**Scope out**: all cryptography, passphrase policy, generation counter and rollback detection (19 and the clients), upload and download mechanics (03), tree structure and `client_meta` storage (02), share links (06 refuses vault files, see below), UI (16, 17).

**No share links for vault files in v1.2** (owner decision, canonical-names). 06 refuses to create a share whose target is a vault file (`TargetNotShareable`), and 20 provides no envelope reader. The link-key idea of the earlier draft is dropped; a later release would need its own design.

## 2. Ports (declared in `contracts`)

**Provides**
| Port | Methods | Consumers |
|---|---|---|
| `VaultService` | `create_vault`, `get_header`, `replace_header`, `report_unlock_failure` | 11 (`/api/v1/vaults/**`) |
| `AccountLifecycleHook` | `initialize(tx, account)` (no-op), `erase_batch(account, limit) -> EraseProgress{remaining}` removes the account's `vaults` rows | 07 `DeletionPurger` drives it; registered by 12 |

**Requires**
| Port | Provider | Used for |
|---|---|---|
| `TreeReader` | 02_tree | the vault root node (must be a live folder of the account) |
| `AuditWriter` (helper 04) | helper | `vault_failure` events |

Vault file commits go through the normal 03/02 commit path with the wrapped key in `client_meta`. How 02/03 learn that a node is under a vault root (decided 2026-11-11, AZ-3913; membership, edge cases and the name form are in section 10): every node carries a nullable `nodes.vault_id`, copied from its parent when the node is created and never changed afterwards, so "is this node in a vault" is answered from the node row alone, with no ancestry walk and no call to 20.

## 3. Internal Interfaces (proposed)

| Operation | Input | Output | Errors |
|---|---|---|---|
| `create_vault` | account, `header: bytes`, root node | `VaultId`, `ETag` | `VaultExists` (one vault per account, decided 2026-11-11; also refused when the root folder is not empty or already a vault root), `HeaderTooLarge`, `NotFound` (unknown, foreign or tombstoned root, a root that is a file, or the account's own root folder: refused even when empty, owner decision 2026-11-13, since every file the account creates would sit in the vault) |
| `get_header` | account, vault id | `header: bytes`, `ETag` | `NotFound` |
| `replace_header` | account, vault id, `header: bytes`, `if_match: ETag` | new `ETag` | `NotFound`, `HeaderTooLarge`, `PreconditionFailed` (the stored header changed since the client read it) |
| `report_unlock_failure` | account, vault id | `()` | none (best-effort; writes audit `vault_failure`, ids only) |

The server treats `header` as opaque bytes with a length cap; it does not parse it. `ETag` is the SHA-256 of the stored header bytes, so two clients replacing the header (passphrase change, recovery) cannot overwrite each other silently: the second one gets 412 and must re-read. The authenticated `generation` counter lives *inside* the header (19); the server cannot read or enforce it, and clients pin the last generation they saw, so the server cannot make a returning client accept an older header. Size limit (100 MiB per vault file, `max_vault_file_bytes`) is checked at commit by 03.

## 4. External API Specification (reserved; routed by 11)
| Endpoint | Method | Auth | Description |
|---|---|---|---|
| `/api/v1/vaults` | POST | SessionOnly | create a vault with its header; returns `ETag` |
| `/api/v1/vaults/{id}` | GET | Session | header, `ETag` header, root node id |
| `/api/v1/vaults/{id}/header` | PUT | SessionOnly | replace the header; requires `If-Match`; 412 on mismatch, 428 when the header is missing |
| `/api/v1/vaults/{id}/unlock-failures` | POST | Session | best-effort failure report |
Paths are proposals; REST naming rules of the project apply. The wrapped file key is read and written through the normal revision routes (`client_meta`), not here.

## 5. Data Access Patterns
**Owned**: `vaults` (`id`, `account_id`, `root_node_id` UNIQUE composite FK to `nodes`, `header` bytea, `created_at`, `updated_at`). The former `vault_files` table (primary key per node) is dropped: a per-node row would hold one key for all revisions, while each revision has its own file key. The table carries `account_id` and is covered by the same row-level policy as the other tenant tables. Queries are primary-key lookups; low volume. No cache. Vault names are plaintext or ciphertext per the open data-model item A6.

## 6. Configuration keys
`max_vault_file_bytes` (100 MiB), `max_vault_header_bytes` (default 1024 bytes; names as in helper 01). Durations: none. The default is the proposed fixed header of 229 bytes (`19_vault_core/vault_format.md`: 53 bytes of authenticated fields, two wrapped keys of 72 bytes, a 32-byte key commitment) plus room for an encrypted vault name field (24-byte nonce, at most 255 bytes of name, 16-byte tag) and a margin of about 4x the fixed part; 1024 and 1025 bytes are the max and max + 1 cases of IT-20-1. If E-29/c01 freezes a longer header, this default is raised to the next power of two above twice the frozen length.

## 7. Security notes
The server holds ciphertext, an authenticated opaque header and wrapped keys only. A stolen database or blob volume yields nothing decryptable without the passphrase or recovery key, but it does allow unlimited offline passphrase guessing against the header (19 section 3 states the residual risk; the product text repeats it, NFT-SEC-43). Every query is scoped by `account_id`; vault routes are in the AC-59 two-account matrix. Failure reports carry ids only and never the passphrase or attempt count text. Metadata (sizes, counts, tree shape, timing) is visible to the server, and the server also chooses the tree shape: it can move, hide or roll back vault files (19 risk 3).

## 8. ACs, tests
**ACs [v1.2]**: AC-13 (server sees ciphertext only: no plaintext path exists in 20), AC-61 (header stored and returned byte-identically; concurrent replacement detected), AC-59 (vault routes in the IDOR matrix). Tests: FT-P-44, FT-P-47, FT-N-13, NFT-SEC-39, NFT-SEC-43; unit: header round-trip byte equality, `If-Match` mismatch gives 412 and leaves the stored header unchanged, missing `If-Match` is refused, deletion hook leaves zero rows, no job enqueued for vault files (`CommitContext.vault`), a share of a vault file is refused by 06.
**Acceptance**: a database dump of a vault account contains no plaintext and no key; account deletion leaves no `vaults` rows.

## 9. Dependency graph
Implement after the review plan is agreed and `19_vault_core`'s byte layout is fixed, and after 02, 03 and 06 ports exist. Parallel with 16 vault page. Blocks FT-P-44..47.

## 10. Risks and open points
1. Decided 2026-11-11 (AZ-3913, data model A6; owner for the two answers, agent proposal for the rest, the owner may overturn): **one vault per account** (`vaults.account_id` UNIQUE; a second create is `VaultExists`; the vault root folder must be new and empty, so no existing content has to be backfilled); **names are ciphertext**: the vault's own name is a field of the opaque header (an encrypted name: nonce, name, tag) and every file and folder name under a vault is encrypted by the client, so the server stores and compares only ciphertext. Because the server folds names with `name_key` (NFC, case folding), E-29 must encode an encrypted name in a case-stable, injective form (for example lower-case base32) so two different names never collide.
2. **Data-model changes this spec needs** (for the data-model owner): drop `vault_files`; `revisions.client_meta` carries the wrapped file key and file header for vault files; `vaults` as in section 5 with `account_id` UNIQUE; nullable `nodes.vault_id` (FK to `vaults`), set when the vault root folder is made (root and every descendant created later copy it from their parent) . Follow-up tasks to add in their owning epics: data model and migration (E-30/c03 with the data-model owner), 02 (`create_folder` and `publish` copy `vault_id` from the parent, move refusal below, E-13), 03 (`start_upload` applies `max_vault_file_bytes` when the parent's `vault_id` is set, E-14), 06 (share create refuses a node whose `vault_id` is set, E-16), E-30/c07 (membership wiring).
3. Vault files get no media jobs: 08's planner reads the `vault` flag of `CommitContext` (set by 02/03), not a dependency on 20.
4. 06 learns "is vault file" from the same `nodes.vault_id`: a node with `vault_id` set is refused as a share target (422 `target_not_shareable`).
5. Membership edge cases (decided 2026-11-11, agent proposal): (a) a node at any depth is a member because `vault_id` is copied from the parent at creation, never walked; (b) **a move across the vault boundary is refused** (into a vault, out of a vault, or from one vault to another) with 409 `vault_boundary`: the server cannot encrypt or decrypt, so a plaintext file would sit inside a vault or a ciphertext file outside it; the client copies instead (download, encrypt, upload, delete); (c) a move between folders of the same vault is allowed and keeps `vault_id`; (d) because of (b), a plaintext file that already has a share link can never enter a vault: it cannot be moved in, and the vault root must be empty at creation; (e) deleting the vault removes its root and every node with its `vault_id`; (f) deleting the vault root folder through 02 tombstones it like any folder, and when 02 purges it after the retention window the vault row is removed with it (`ON DELETE CASCADE`, owner decision 2026-11-13), so the account can create a new vault.
