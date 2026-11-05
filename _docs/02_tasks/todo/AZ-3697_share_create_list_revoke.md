# ShareService: create, list and revoke share links

**Task**: AZ-3697_share_create_list_revoke
**Name**: Share link owner operations
**Description**: Let a file owner create a revocable link with a default 7-day expiry and an optional password, list their links without tokens, and revoke them, storing only hashes
**Complexity**: 3 points
**Dependencies**: AZ-3696_sharing_crate_migration_config, AZ-3764_sc_error, AZ-3765_sc_clock, AZ-3766_sc_ids, AZ-3784_sc_audit, AZ-3707_tree_reader
**Component**: 06 sharing
**Tracker**: AZ-3697
**Epic**: AZ-3619 (E-16)
**Ref**: E-16/c03

## Problem

v1.0 sharing between users is by link only, and links target files, not folders (decision 15). The link token travels in a URL, so the server must never be able to show it again or leak it: only its hash is stored, and it is shown once at creation (AC-12). An owner must not be able to probe other accounts' node ids through link creation.

## Outcome

- `ShareService::create`, `list`, `revoke` on the port defined in `contracts` (E-00/c05).
- Tokens `sc_shr_` plus 256 random bits; only SHA-256 stored; an optional password stored as an Argon2id PHC string.
- Audit events `share_created` and `share_revoked` with ids only.

## Scope

### Included
- `create(account, node id, expires_in?, password?)`: target resolved through `TreeReader::resolve_live_file`; a folder gives `TargetNotShareable`; a gone node (other account, tombstoned, purged, random id) gives one `TargetNotFound` value; expiry defaults to `share_default_ttl` (7 d), must be above 0 and at most `share_max_ttl` (365 d), else `ExpiryOutOfRange`; result `CreatedShare{id, token, url_path, expires_at, has_password}` with the token only here.
- `list(account, node?)`: `ShareInfo` of the account's links, optionally for one node, never a token.
- `revoke(account, share id)`: sets `revoked_at`; idempotent for the owner; `NotFound` for another account's or an unknown id.
- `share_created`, `share_revoked` audit events.

### Excluded
- The session-only check on owner routes (01 `Authenticator::require` at the boundary, E-21; checked by E-15/c13).
- Visitor operations (E-16/c04 to c06); abuse limits (E-16/c07).
- The vault-file refusal (v1.2; owner item "Vault membership", E-30).

## Acceptance Criteria

**AC-1: Create with the default expiry (AC-12)**
Given a live file f of account A and `share_default_ttl = 7d`
When A creates a link on f without expiry or password
Then the result has a token starting `sc_shr_`, a `url_path`, `expires_at` = now + 7 d and `has_password = false`

**AC-2: Expiry range**
Given f
When links are created with `expires_in` 1 h, 365 d, 366 d, 0 and a negative value
Then 1 h and 365 d are accepted with `expires_at` = now + value; 366 d, 0 and negative give `ExpiryOutOfRange`

**AC-3: Target rules**
Given a folder d of A, a tombstoned file, a purged file, a file of account B and a random id
When A creates a link on each
Then the folder gives `TargetNotShareable`; the other four give the same `TargetNotFound` value

**AC-4: Only hashes at rest**
Given a link created with password `correct horse`
When every column of `share_links` is read with the migrator role
Then `token_hash` equals SHA-256 of the token, the plaintext token appears in no column, and `password_hash` is an Argon2id PHC string with no plaintext password anywhere

**AC-5: List shows own links without tokens**
Given A with links on f and g, and B with a link
When A lists all links and links for node f
Then A sees its two links (one for the node filter), each without a token, and never B's link

**AC-6: Revoke is idempotent for the owner and closed to others**
Given A's link L and B's link M
When A revokes L twice, A revokes M, and A revokes a random id
Then both revokes of L succeed and L has `revoked_at`; revoking M and the random id give `NotFound` and M is unchanged

**AC-7: Owner events are audited with ids only**
Given the creates and revokes above
When the audit log is read
Then each create wrote one `share_created` and each first revoke one `share_revoked`, with link, node and account ids only, and no token, file name or password

## Non-Functional Requirements

**Reliability**
- Password hashing uses the configured Argon2id parameters; create is not on the visitor hot path.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Token format | Prefix `sc_shr_`, 32 random bytes encoded |
| AC-2 | Expiry validation boundaries | (0, `share_max_ttl`] accepted, else `ExpiryOutOfRange` |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Live file, 7 d default | IT-06-1 default create | Token, url path, +7 d, no password | FT-P-37 |
| AC-2 | Live file | IT-06-1 expiry cases | Accepted or `ExpiryOutOfRange` | FT-P-37 |
| AC-3 | Folder, tombstoned, purged, foreign, random | IT-06-1 target cases | `TargetNotShareable`; identical `TargetNotFound` | FT-P-80 |
| AC-4 | Link with password | IT-06-1 storage | Hash only; Argon2id PHC | FT-P-38, NFT-SEC-16 |
| AC-5 | Links of A and B | IT-06-1 list | Own links, no token | ST-06-1 |
| AC-6 | L of A, M of B | IT-06-1 revoke | Idempotent; foreign and unknown `NotFound` | ST-06-1 |
| AC-7 | Creates and revokes | IT-06-11 owner events | One event each; ids only | |

## Runtime Completeness

- **Named capability**: Argon2id password hashing (`argon2` crate, PHC strings) and the E-04 token generator.
- **Production code that must exist**: real Argon2id hashing with the configured parameters and a unique salt per link.
- **Allowed external stubs**: none.
- **Unacceptable substitutes**: storing a fast hash, a fixed salt, or lowered cost in production builds.

## Constraints

- Tests run against the real tree (E-13) on PostgreSQL 18; no fake store.
- A link follows the node id, so later rename and move need no handling here.

## Risks & Mitigation

**Risk 1: Node-id probing through create**
- *Risk*: different errors reveal whether another account's node exists.
- *Mitigation*: one `TargetNotFound` value for every gone case (AC-3).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| How 06 learns that a file is a vault file (to refuse it in v1.2) is an open owner item ("Vault membership", E-30); v1.0 has no vaults | Product owner / E-30 | open | Low |
