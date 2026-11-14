# Constant-work token lookup and the visitor open outcomes with one NotFound

**Task**: AZ-3698_share_lookup_open_uniform
**Name**: Share link lookup and open
**Description**: Resolve a visitor's link token with the same work in every case and answer Available, PasswordRequired or one indistinguishable NotFound, showing only name, size, type and expiry
**Complexity**: 5 points
**Dependencies**: AZ-3695_sharing_gaps_grant_key_spike, AZ-3697_share_create_list_revoke, AZ-3707_tree_reader, AZ-3708_tree_folder_mutations, AZ-3709_tree_publish_core, AZ-3714_tree_cascade_purge_janitors
**Component**: 06 sharing
**Tracker**: AZ-3698
**Epic**: AZ-3619 (E-16)
**Ref**: E-16/c04

## Problem

A visitor must learn nothing about whether a token exists, expired, was revoked or points at a deleted file: status, body, headers and median timing must match within 1 ms (AC-12). The lookup runs before the owner is known, so it is the single place in sharing that reads across accounts. A password-protected link must reveal nothing about its file before unlock (NFT-SEC-51).

## Outcome

- One lookup function: one indexed read by `token_hash` in system scope, then decisions in code with the same follow-up reads in every case.
- `PublicShareService::open(token, ip)` returns `Available{name, size, media_type, expires_at}`, `PasswordRequired` or `NotFound`.

## Scope

### Included
- The lookup: hash the presented token, read by `token_hash` in system scope (confined to this function), then decide live / expired / revoked in code, with the same target reads (`TreeReader::resolve_live_file` and `get_node`, two primary-key reads) or equivalent padding when the branch ends early.
- `open`: live link without password → `Available` with name from `get_node` and size and media type of the target's current revision; live password link → `PasswordRequired` with no file data; every other case → `NotFound`.
- Expiry boundary (G-06-3, decided 2026-11-07): `expires_at > now` is live, `expires_at <= now` is `NotFound`; `now` from the injectable clock.
- Malformed tokens (empty, 10 kB, non-ASCII) take the same path and answer `NotFound`.

### Excluded
- The 1 s result cache (E-16/c06 adds it for both `open` and `authorize_content`).
- Invalid-token counting and `RateLimited` (E-16/c07); password verification (E-16/c05).
- HTTP status 401 `{code: "password_required"}` and byte-identical 404 responses (E-21/c14).

## Acceptance Criteria

**AC-1: A live link shows its current file (AC-12, AC-47)**
Given a live link on file f
When a visitor opens it, the owner saves a new revision in place, and the visitor opens it again
Then both answers are `Available` with f's name, and size and media type of the current revision (the media type as sniffed on upload), the second answer showing the new revision's values

**AC-2: A password link reveals nothing before unlock (NFT-SEC-51)**
Given a live link with a password on `secret-plan.pdf`
When a visitor opens it
Then the answer is `PasswordRequired` carrying no name, size, media type or expiry

**AC-3: One NotFound for every dead case (AC-12)**
Given links that are expired (clock), revoked, on a tombstoned target, on a purged target (row cascaded), an expired password link, an unknown token, and malformed tokens `""`, 10 kB and non-ASCII
When a visitor opens each
Then every answer is the same `NotFound` value with the same serialisation

**AC-4: Constant work (IT-06-9)**
Given the statement log for `open` in six cases: unknown, expired, revoked, tombstoned, purged, live
When the five NotFound cases are compared
Then they show the same number and shape of statements (one indexed read by `token_hash`, then the same resolve reads or padding), and none calls Argon2

**AC-5: The visitor result has four fields only**
Given the `Available` type
When its fields are listed
Then they are exactly name, size, media type and expiry, with no account id, owner, path or node id

**AC-6: The cross-account read is confined**
Given the sharing crate
When the code paths that enter system scope are listed
Then the token lookup is the only one, and it returns at most one row for an exact `token_hash`

## Non-Functional Requirements

**Performance**
- `open` p95 at most 10 ms (fail above 50 ms) and timing difference between missing cases median at most 0.3 ms (fail above 1 ms): PT-06-1 and PT-06-2, run by E-16/c09 and on REF-SRV by E-16/c10.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Decision function over (row state, target state, clock) | `NotFound` for every non-live combination |
| AC-5 | Field list of `Available` | Exactly 4 fields |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Live link; save-in-place edit | IT-06-2 live and edit cases | `Available` with current revision values | FT-P-36, FT-P-57 |
| AC-2 | Password link | IT-06-2 password case | `PasswordRequired` without file data | NFT-SEC-51, FT-P-37 |
| AC-3 | Expired, revoked, tombstoned, purged, unknown, malformed | IT-06-2 | One `NotFound` value | FT-N-09, NFT-SEC-17 |
| AC-4 | Six cases, statement log | IT-06-9 | Same statements in the five NotFound cases; no Argon2 | PT-06-2 |
| AC-5 | `Available` type | ST-06-7 field list (run in E-16/c09) | Four fields | |
| AC-6 | Crate source and DB | ST-06-8 (run in E-16/c09) | Single system-scope path; exact-hash only | |

## Constraints

- The single cross-account token resolution in system scope is the documented exception of ADR-014 and data model §12.3; nothing else in 06 sets system scope.
- Tests run against the real tree (E-13) on PostgreSQL 18.

## Risks & Mitigation

**Risk 1: Timing difference from early returns**
- *Risk*: an unknown token skips the target reads and answers faster.
- *Mitigation*: the same reads or padding in every branch (AC-4); measured on REF-SRV (E-16/c10).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| What the padding reads for an unknown token are (for example a fixed dummy node lookup) is not described; IT-06-9 only requires the same statement count and shape | E-16 owner | closed 2026-11-14: a link that is not live (unknown, malformed, purged, expired, revoked) reads the fixed dummy target (nil account, nil node) through the same `resolve_live_file` and `get_node` calls; the five not-found cases log the same three data statements | Low |
| A live target makes 02's `resolve_live_file` and `get_node` each add the ancestor walk (`WITH RECURSIVE chain`), so a live link runs five data statements against three for every not-found case, and a file under a tombstoned folder whose cascade is still pending runs five and answers `NotFound`. The dummy reads cannot reproduce the walk through `TreeReader`; equal counts with the live case need a 02 change (for example the walk run unconditionally) | 02 tree owner | open | Low |

## Owner decision

Owner decision 2026-11-14 (complexity budget, option A): an unknown or malformed token takes fixed dummy primary-key reads with the same statement count and shape as a live link; Argon2 password verification runs on the blocking pool, not the async worker.

## Implementation notes (2026-11-14)

- Lookup: `src/components/06_sharing/src/lookup_repo.rs::by_token_hash`, the only `run_system` call of 06 (`share_lookup`). It needs the allowlist line `src/components/06_sharing/src/lookup_repo.rs` in `scripts/ci/run-system-allowlist.txt`.
- `open` is an inherent method of `PgShareService` with the port's signature; `impl PublicShareService` follows when `verify_password` (E-16/c05) and `authorize_content` / `authorize_derived` (E-16/c06) exist, so no operation is stubbed.
- Link liveness is decided before the target reads; a link that is not live reads the dummy target, so expired and revoked links cost what an unknown token costs.
- Share create hashes the password on tokio's blocking pool (`spawn_blocking`).
