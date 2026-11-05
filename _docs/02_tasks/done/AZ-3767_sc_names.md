# sc_names: legal names, name keys, conflict-copy names, portability and escaping

**Task**: AZ-3767_sc_names
**Name**: sc_names validation, `name_key`, conflict-copy names, portability and escaping, shared corpus
**Description**: A pure crate that defines a legal name, the case-insensitive key, the server-assigned conflict-copy name and what a client does with a name its file system cannot hold, with one shared hostile-name corpus
**Complexity**: 5 points
**Dependencies**: AZ-3765_sc_clock, AZ-3652_initial_structure
**Component**: shared/names (helper 06)
**Tracker**: AZ-3767
**Epic**: AZ-3636 (E-05)
**Ref**: E-05/c01

## Document Dependencies

- `_docs/02_document/contracts/shared_clock/sc_clock.md` — `Timestamp` for `conflict_copy_name`, produced by E-03/c01.

## Problem

Names must be unique per folder case-insensitively and stored case-preserving (AC-42). Hostile names (traversal, alternate data streams, reserved Windows names, bidi characters) must cause 0 writes outside the sync root (AC-37). Conflict copies must be named by one function so the server decides and clients never retry with another name. If server and clients folded names differently, files would collide or duplicate across devices.

## Outcome

- The server accepts every name except a short, exact list.
- One fold (`name_key`) with a pinned, reported Unicode version decides uniqueness everywhere.
- Conflict-copy names always fit 255 bytes, keep the extension and pass validation.
- Clients get deterministic portability reasons and escaped local names from one corpus shared by server, daemon and web tests.

## Scope

### Included
- `validate_name` with exactly the rejected classes of helper 06; everything else accepted.
- `name_key` (NFC then Unicode full case folding) and `NAME_KEY_VERSION`.
- `conflict_copy_name`: format, UTC time, label sanitising, truncation on a character boundary within 255 bytes (label cut first), ` (2)`, ` (3)` before the extension on collision.
- `split_path` that never yields `.`, `..` or a component with a separator.
- `portable_check` and `escape_name` for the three file-system profiles; `%XX` escaping; the open defaults (`%5F` marker, ` (2)` suffix) as named constants in one place.
- `NameLookup` (is this name taken in the folder) for database-backed or in-memory callers.
- The shared corpus `names_corpus.json` with 40 hostile cases per OS, referenced (not copied) by the daemon and web tests.
- Choice and pin of a Unicode normalisation and case-folding library whose version is reported.

### Excluded
- Symlink, junction and reparse-point defence (14 `SafeFs`, E-24); file-system profile detection (E-24); batch re-key after a Unicode change (later release).

## Acceptance Criteria

**AC-1: Exact accept and reject list (AC-37)**
Given names covering every rejected class (`.`, `..`, `/`, NUL, each Cc range, each listed bidi control, empty, 256 bytes) and accepted samples (`CON`, `a.`, `a `, `a\b`, `a:b`, `-x`, emoji, mixed case)
When `validate_name` runs on each
Then exactly the rejected classes fail with their named error and every accepted sample passes (FT-N-41 unit part)

**AC-2: Case-insensitive uniqueness (AC-42)**
Given `Readme.md` and `README.MD`, and an NFC and an NFD spelling of one name
When `name_key` runs on each
Then each pair yields one key, and the validated name keeps its original case

**AC-3: Conflict copy fits (AC-8)**
Given a multi-byte stem near 255 bytes, a long device label with a `/` and a bidi control, and an extension
When `conflict_copy_name` runs
Then the result is at most 255 bytes, keeps the extension, has the label sanitised, passes `validate_name`, and with the result already taken in the folder the next call inserts ` (2)` before the extension

**AC-4: Hostile corpus (AC-37)**
Given the 40-case corpus for each OS profile
When `split_path`, `portable_check` and `escape_name` process every case against an in-memory folder
Then 0 results contain a traversal component, and 0 escaped names collide with an existing name in the folder (FT-N-19 unit part)

**AC-5: Deterministic escaping**
Given `a:b` and `CON.txt` on the Windows profile and the same folder state twice
When `escape_name` runs
Then it returns `a%3Ab` and `CON%5F.txt` both times

**AC-6: Pure and portable**
Given the crate
When it is built for `wasm32-unknown-unknown` and its dependency list is read
Then the build exits 0 and it has no I/O, database or async runtime dependency

**AC-7: Properties hold**
Given random names
When the property tests run
Then `validate_name(conflict_copy_name(x))` always succeeds and `name_key` is idempotent

## Non-Functional Requirements

**Performance**
- `validate_name` plus `name_key` under 5 microseconds for a typical name.

**Compatibility**
- Names are at most 255 bytes of UTF-8 (assumption A1); `name_key` is computed in Rust only.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Every rejected and accepted class | Exact list |
| AC-2 | Fold stability for one `NAME_KEY_VERSION` | Same key across calls |
| AC-3 | Truncation with multi-byte stem; label sanitising; numeric suffix | Rules hold |
| AC-4 | Corpus over `split_path` and `escape_name` | 0 traversal, 0 collisions |
| AC-5 | Repeat escaping | Same result |
| AC-7 | Property tests | Always valid; idempotent |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Server API (through E-13, E-21) | FT-N-41, NFT-SEC-22 | Server rejects exactly the list | — |
| AC-4 | 40-case corpus on Linux and Windows clients (through E-24) | FT-N-19, NFT-SEC-31 | 0 writes outside the root, 0 overwrites | — |
| AC-2 | Case-only rename (through E-13) | FT-P-15 | File id kept | — |
| AC-1 | Hostile names in the tree (through E-13) | FT-N-04, FT-N-05 | Handled per the server rules | — |

## Constraints

- ADR-005 (last write wins with conflict copies); RESTRICT-07 (unique per folder case-insensitively, case-preserving); R16 (fold version is user-visible).
- Depends on `sc_clock` for `Timestamp` only; no `sc_db`.

## Risks & Mitigation

**Risk 1: A Unicode table update changes keys (R16)**
- *Risk*: a library bump changes `name_key` of stored names.
- *Mitigation*: the library and its Unicode version are pinned; `NAME_KEY_VERSION` is stored per node; re-key is a later release.

**Risk 2: Server and daemon corpora diverge**
- *Risk*: two copies drift.
- *Mitigation*: one corpus file, referenced by the other tests.

## Runtime Completeness

- Named capability: Unicode normalisation (NFC) and full case folding with a pinned Unicode version.
- Production code that must exist: a real normalisation and case-folding library, pinned, with its Unicode version reported by `NAME_KEY_VERSION`.
- Allowed external stubs: none.
- Unacceptable substitutes: ASCII lower-casing, `to_lowercase` without normalisation, or an unpinned library.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The Windows reserved-name marker (`%5F`) and the ` (2)` escape suffix are open owner items; they are named constants with tests that assert the current choice. | Project owner | open | Low |
| NFC versus NFD as the stored form is an owner item routed through E-13's spike (G-1). | Project owner, E-13 | open | Medium |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/shared_names/sc_names.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
