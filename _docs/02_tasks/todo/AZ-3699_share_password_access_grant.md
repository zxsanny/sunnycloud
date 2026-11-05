# Share password verification with a bounded Argon2id pool and the signed access grant

**Task**: AZ-3699_share_password_access_grant
**Name**: Share password and access grant
**Description**: Check a visitor's share password at a bounded cost and return a short-lived access grant signed with the share grant key that only that link accepts and that cannot be forged, moved or extended
**Complexity**: 5 points
**Dependencies**: AZ-3700_share_authorize_content, AZ-3697_share_create_list_revoke, AZ-3763_sc_config, AZ-3766_sc_ids
**Component**: 06 sharing
**Tracker**: AZ-3699
**Epic**: AZ-3619 (E-16)
**Ref**: E-16/c05

## Problem

A password link must open only after the right password. Argon2id is expensive on purpose, so a flood of guesses could exhaust the CPU (NFT-SEC-06), and a guess against a token that does not exist must cost nothing. After unlock the visitor's browser needs a proof for later content requests that holds no server state, binds to that link only and expires (decision 15).

## Outcome

- `PublicShareService::verify_password(token, password, ip)` returns `AccessGrant{value, expires_at}`, `WrongPassword` or `NotFound`.
- At most `share_password_max_concurrency` Argon2id verifications run at once.
- The grant is `HMAC(share_grant_key, token_hash ‖ expires_at)` with `share_grant_ttl` (10 min); `authorize_content` (E-16/c06) accepts a valid grant for a password link.

## Scope

### Included
- `verify_password`: the lookup of E-16/c04 first; a token that is not a live password link answers `NotFound` without taking a permit or hashing; otherwise take a permit, verify the Argon2id PHC string, release; right password → grant; wrong → `WrongPassword`.
- Grant issue and grant check: value bound to the link's `token_hash` and its `expires_at`; compared in constant time; expired, tampered, empty, oversized or other-link grants are invalid.
- Grant acceptance inside `authorize_content`: a password link with a valid grant is allowed; without one it stays `PasswordRequired`; links without password ignore grants.

### Excluded
- The wrong-password counter and `RateLimited` (E-16/c07).
- Sending the grant as an HttpOnly path-scoped cookie (E-21/c14).
- Password hashing at create (E-16/c03).

## Acceptance Criteria

**AC-1: Right password gives a grant, wrong does not (AC-12)**
Given link L1 with a password, `share_grant_ttl = 500 ms`, and Unicode and 1 kB passwords on two more links
When the right and a wrong password are sent to each
Then the right password returns `AccessGrant` with `expires_at` = now + 500 ms, and a wrong one returns `WrongPassword`

**AC-2: No hash for a token that does not exist**
Given a nonexistent token
When `verify_password` is called
Then it returns `NotFound` in under 2 ms and no semaphore permit is taken

**AC-3: A grant opens only its own link while valid**
Given links L1 and L2 with different passwords and a grant from L1
When `authorize_content` runs on L1 with the grant, on L2 with it, on L1 after the grant's expiry, on L1 with one byte of the grant changed, and on a link without password with and without the grant
Then L1 with the valid grant is allowed; L2, the expired and the tampered grant give `PasswordRequired`; the no-password link is allowed either way

**AC-4: Grants are stateless and die with the link**
Given a valid grant for L1
When L1 is revoked and `authorize_content` runs with the grant
Then no grant row exists anywhere in the database, and the call returns `NotFound`

**AC-5: Hash cost is bounded**
Given `share_password_max_concurrency = 4`
When 50 wrong-password calls run at once
Then at most 4 verifications are in progress at any moment

**AC-6: Grants cannot be forged or replayed elsewhere (ST-06-4)**
Given 1,000 mutated grants (a bit flipped at every byte offset, a changed `expires_at`, the grant of another link, empty and oversized values)
When each is presented to `authorize_content` for L1 and L2
Then all give `PasswordRequired`, and the median timing of a valid-prefix and an invalid grant differs by at most 1 ms

**AC-7: Passwords are stored with Argon2id and unique salts (ST-06-5)**
Given 100 password links
When their `password_hash` values are parsed
Then 100% are `argon2id` PHC strings with the configured memory, iterations and parallelism, and no two share a salt

## Non-Functional Requirements

**Performance**
- During a 1,000-concurrent wrong-password flood, `open` p95 at most 50 ms (fail above 200 ms) and hashing CPU at most permits x 1 core (PT-06-3, run by E-16/c09).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Grant encode and verify | Valid only for the same `token_hash` and before `expires_at` |
| AC-6 | Comparison | Constant-time compare used for the grant MAC |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | L1, Unicode and 1 kB passwords; ttl 500 ms | IT-06-3 right and wrong | Grant with ttl; `WrongPassword` | FT-N-10 |
| AC-2 | Nonexistent token | IT-06-3 no-hash case | `NotFound` < 2 ms; no permit | NFT-SEC-06 |
| AC-3 | L1, L2, no-password link | IT-06-3 grant matrix | As stated | NFT-SEC-51 |
| AC-4 | Grant for L1; revoke | IT-06-3 stateless case | No grant row; `NotFound` | |
| AC-5 | Concurrency 4; 50 callers | Permit count | At most 4 in progress | PT-06-3 |
| AC-6 | 1,000 mutated grants | ST-06-4 | All `PasswordRequired`; timing within 1 ms | |
| AC-7 | 100 password links | ST-06-5 | argon2id, configured params, unique salts | NFT-SEC-06 |

## Runtime Completeness

- **Named capability**: Argon2id verification (`argon2`), HMAC-SHA-256 grants (`hmac`, `sha2`), constant-time comparison (`subtle`).
- **Production code that must exist**: real Argon2id verification against the stored PHC string; real HMAC with the configured `share_grant_key`.
- **Allowed external stubs**: none.
- **Unacceptable substitutes**: plain string comparison of passwords or grants, a fixed test key in release builds, or skipping verification when the pool is busy.

## Constraints

- The grant key is a secret loaded by E-16/c02; it never appears in logs.
- No grant table: revocation works because every content request re-reads the link (E-16/c06).

## Risks & Mitigation

**Risk 1: CPU exhaustion through password floods (R: hash-cost DoS)**
- *Risk*: many parallel guesses saturate every core.
- *Mitigation*: the semaphore (AC-5) plus the per-link limit (E-16/c07); measured by PT-06-3.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `verify_password` on a live link that has no password: outcome not stated | E-16 owner | open | Low |
| Argon2id parameters and `share_password_max_concurrency` have no stated defaults (see E-16/c02) | E-16 owner | open | Medium |
| This task follows E-16/c06 so that the grant check lands in an existing `authorize_content`; the epic lists the rows in the other order | E-16 owner | resolved | Low |
