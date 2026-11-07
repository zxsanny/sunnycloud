# Test Specification — 06_sharing

Tier: component integration (`cargo test -p sharing`) at the public port boundary: `ShareService` (`create`, `list`, `revoke`), `PublicShareService` (`open`, `verify_password`, `authorize_content`), `AccountLifecycleHook`. Env class `C`; timing rows `C-REF`. System scenarios are referenced by ID, not restated. Byte streaming, headers and session-only scope are 03/11/01 (see AC-57 row).

06 reads the tree only through `TreeReader` (`resolve_live_file`, `get_node`); the tests run it against the real 02 and real 05/07a on PostgreSQL 18 (no fake store). `AbuseLimiter` is the real helper 08 with millisecond windows. `AuditWriter` is real (rows read back with the API role's SELECT); a failing-audit decorator exists only for IT-06-11. Clock is the manual test clock; `share_default_ttl`, `share_lookup_cache` (200 ms), `share_grant_ttl`, limit windows are millisecond values unless a row asserts the production default.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion (short) | Test IDs | Coverage |
|-------|------------------------------|----------|----------|
| AC-12 | Link opens <= 2 s, 7-day default, revocation <= 5 s, hashed tokens, uniform missing/expired/revoked | IT-06-1, IT-06-2, IT-06-6, IT-06-7, IT-06-9, PT-06-1, PT-06-2, ST-06-2, ST-06-3, AT-06-1 | Covered here + FT-N-09, FT-N-10, FT-P-36, FT-P-37, FT-P-38, NFT-SEC-16, NFT-SEC-17 |
| AC-20 | Share password and invalid-token limits | IT-06-8, ST-06-3, ST-06-6 | Covered here + NFT-SEC-04, NFT-SEC-06 |
| AC-34 | Link survives save-in-place | IT-06-4, AT-06-3 | Covered here + FT-P-57 |
| AC-47 | Served-type rules shared with 11 | IT-06-2 (media type from sniffed revision) | Covered here (decision input) + FT-N-11, FT-P-39, NFT-SEC-19 |
| AC-57 | Create-share is session-only | — | Covered by system scenarios FT-N-24, NFT-SEC-07 (scope check is 01 `Authenticator::require` + 11; `ShareService` receives a plain `AccountId`) |
| AC-59 | Two-account matrix | ST-06-1, ST-06-8 | Covered here + NFT-SEC-13 |
| AC-27 | Account deletion removes links | IT-06-10 | Covered here + FT-P-19, NFT-SEC-27 |
| AC-77 | Link follows rename/move; delete = missing within 5 s | IT-06-5, IT-06-6, AT-06-3 | Covered here + FT-N-44 (see gap G-06-1) |
| AC-69, AC-70, AC-74 | Interaction ACs where 06 is not an owner | — | Covered by system scenarios NFT-RES-35, FT-N-42, NFT-RES-38 (06 holds no commit or quota path) |

## Blackbox Tests

### IT-06-1: Create, list, revoke
**Summary**: link lifecycle on files with correct storage and errors.
**Traces to**: AC-12, AC-20
**Description**: file f (live), folder d, tombstoned file, purged file, other account's file; `share_default_ttl = 7d` for this test (production default asserted by a config read, not by waiting).
**Input data**: `create(account, node, expires_in?, password?)` for: f default; f with 1 h; f with 366 d; f with 365 d; f with 0 and negative; f with password `correct horse`; d; tombstoned; purged; foreign; random id.
**Expected result**:
```
CreatedShare{id, token "sc_shr_...", url_path, expires_at = now + ttl, has_password}; default expires_at = now + 7 d; 365 d accepted, 366 d / 0 / negative -> ExpiryOutOfRange
folder -> TargetNotShareable; tombstoned / purged / foreign / random -> TargetNotFound (identical value)
DB: token_hash = SHA-256(token) UNIQUE, plaintext token absent from every column; password_hash is an Argon2id PHC string; no plaintext password
list(account) -> ShareInfo without token; filter by node; never another account's links; revoke -> ok, again -> ok (idempotent); revoke foreign/unknown id -> NotFound
```
**Max execution time**: 8 s
**Dependencies**: PostgreSQL, real 02

### IT-06-2: open outcomes
**Summary**: visitor `open` answers Available / PasswordRequired / one NotFound.
**Traces to**: AC-12, AC-47
**Input data**: live link, live password link, expired (clock), revoked, target tombstoned, target purged (row cascaded), unknown token, malformed token (`""`, 10 kB, non-ASCII), password link that is also expired.
**Expected result**:
```
live: Available{name, size, media_type, expires_at} (media type of the CURRENT revision as sniffed by 03; name from get_node); password link: PasswordRequired
every other case, including expired password link: NotFound; the NotFound values are equal (same enum value, same serialisation)
after a save-in-place edit, size and media_type reflect the new revision
```
**Max execution time**: 5 s
**Dependencies**: PostgreSQL, real 02

### IT-06-3: Password verification and access grant
**Summary**: correct password yields a link-bound, expiring grant; wrong password and missing token do not cost an Argon2 hash.
**Traces to**: AC-12, AC-20
**Description**: link L1 with password, link L2 with another password, `share_grant_ttl = 500 ms`, `share_password_max_concurrency = 4`.
**Input data**: right/wrong password on L1; grant from L1 presented to L2; grant after ttl; grant with one byte changed; verify on a nonexistent token; unicode and 1 kB passwords.
**Expected result**:
```
right -> AccessGrant{value, expires_at = now + ttl}; wrong -> WrongPassword; nonexistent token -> NotFound with NO hash computed (elapsed < 2 ms; semaphore permits never taken)
authorize_content: L1 grant on L1 -> ok; L1 grant on L2 -> PasswordRequired; expired or tampered grant -> PasswordRequired; link without password ignores grants
grant is stateless: no grant table row appears; revoking L1 makes the grant useless at once (next authorize_content -> NotFound)
```
**Max execution time**: 8 s
**Dependencies**: PostgreSQL

### IT-06-4: authorize_content returns the current revision
**Summary**: each content request re-reads link and target and returns the live current revision.
**Traces to**: AC-34, AC-12
**Description**: link on f (r1); `authorize_content(token, grant?, ip, starts_download)`; then 02 `publish` of r2 (save-in-place, same node); audit rows counted.
**Input data**: calls with `starts_download = true` (no Range or range from 0), `= false` (range from byte 1,000), 5 calls in a row.
**Expected result**:
```
ShareContent{account_id, node_id, revision_id = r1}, after the edit revision_id = r2 within share_lookup_cache; the link row is untouched (same id, same token)
exactly one share_accessed audit event per call with starts_download = true; 0 for false; event detail holds ids only
no-password link without a grant -> ok; password link without grant -> PasswordRequired
```
**Max execution time**: 6 s
**Dependencies**: PostgreSQL, real 02

### IT-06-5: Link follows rename and move
**Summary**: the node id is the target, so rename and move keep the link working.
**Traces to**: AC-77
**Description**: f in /a/f.txt; link L. Rename to `g.txt`, move to /b/, case-only rename `G.TXT`, move into a deeper folder.
**Expected result**: after each step `open` shows the new name and `authorize_content` returns the same `node_id` and the current `revision_id`, same token; 0 changes to `share_links`.
**Max execution time**: 6 s
**Dependencies**: PostgreSQL, real 02

### IT-06-6: Delete, revive and purge
**Summary**: deleting the file makes the link answer missing within the cache window; revival restores it; purge removes the row.
**Traces to**: AC-77, AC-12
**Description**: `share_lookup_cache = 200 ms`; delete f, poll `authorize_content` every 20 ms; revive by `publish Existing`; delete folder containing a shared file and poll; advance clock beyond `tombstone_horizon` and run `purge_tombstones`.
**Expected result**:
```
After delete_node(f): NotFound within 200 ms + 100 ms (spec bound 5 s); open also NotFound
After revival (edit beats delete): link works again with the same token (row never deleted)
Folder delete (one tombstone, descendants pending cascade): the shared file answers NotFound within 5 s, also BEFORE cascade_batch has run -> this expectation can fail (gap G-06-1); the test records the elapsed time
After purge: share_links row gone (ON DELETE CASCADE); NotFound
```
**Max execution time**: 20 s
**Dependencies**: PostgreSQL, real 02/05

### IT-06-7: Expiry and revocation timing
**Summary**: expiry follows the injectable clock; revoke is visible within the cache.
**Traces to**: AC-12
**Description**: link with `expires_in = 1 h`; clock to `expires_at - 1 ms`, `expires_at`, `expires_at + 1 s`; revoke while `authorize_content` is polled.
**Expected result**: live before `expires_at`; NotFound at and after (boundary per description §2 and §5: `expires_at > now` is live, `expires_at <= now` is NotFound; G-06-3 resolved 2026-11-07); revoke -> NotFound within `share_lookup_cache + 100 ms` and always within 5 s; no product timestamp comes from SQL `now()` (clock advance moves the answer).
**Max execution time**: 5 s
**Dependencies**: PostgreSQL

### IT-06-8: Abuse limits
**Summary**: invalid-token and wrong-password counters trip at the stated numbers.
**Traces to**: AC-20
**Description**: windows scaled to 2 s: 100 invalid tokens/min/IP, 10 wrong passwords/link/10 min.
**Input data**: 100 then 101 `open` with unknown tokens from IP1; same from IP2; 10 then 11 wrong passwords on L1 from IP1, then from IP2, then on L2; correct password during lockout; clock past the window.
**Expected result**:
```
invalid tokens: calls 1..100 NotFound, 101st RateLimited{retry_after > 0}; IP2 unaffected; counter resets after the window
wrong passwords: 11th attempt RateLimited{retry_after}; counter is per LINK (IP2 on L1 also limited); L2 unaffected; correct password while limited -> RateLimited, no Argon2 verification runs (description §5; G-06-2 resolved 2026-11-07)
each trip writes ONE audit event (not one per rejected call); WARN log carries link id or IP hash, never the token
```
**Max execution time**: 15 s
**Dependencies**: PostgreSQL, helper 08

### IT-06-9: Constant-work lookup
**Summary**: every not-found case runs the same statements.
**Traces to**: AC-12
**Description**: capture the statement log for `open` and `authorize_content` in six cases: unknown, expired, revoked, tombstoned, purged, live.
**Expected result**: the five NotFound cases show identical statement count and shape (one indexed read by `token_hash`, then the same resolve reads; padded where the branch ends early); the response values are equal; no Argon2 call in any of them.
**Max execution time**: 5 s
**Dependencies**: PostgreSQL

### IT-06-10: Account erase hook
**Summary**: links are deleted in batches and stop resolving.
**Traces to**: AC-27
**Description**: account with 120 links; `erase_batch(limit = 50)` loop; `initialize` no-op.
**Expected result**: `remaining` 70, 20, 0; links NotFound after their row is gone and while the node is a tombstone; other accounts' links untouched; running again returns `remaining = 0`.
**Max execution time**: 8 s
**Dependencies**: PostgreSQL

### IT-06-11: Audit events and audit failure
**Summary**: events carry ids only; audit failure does not leak into the response.
**Traces to**: AC-12
**Description**: create, revoke, access with the real writer; then the decorator makes `AuditWriter` fail.
**Expected result**: `share_created`, `share_revoked`, `share_accessed` rows with link id/node id/account id in `detail`, no token, name or password; with the failing writer the service call still returns its normal result and an ERROR is logged.
**Max execution time**: 5 s
**Dependencies**: PostgreSQL

### IT-06-12: Grant key required at start
**Summary**: `ShareConfig` validation refuses a missing or unusable `share_grant_key`; `serve` calls it before it binds the listener.
**Traces to**: AC-12
**Description**: component test against the `ShareConfig` validation of the sharing crate (description §5), the single entry point. Cases: key missing; key file unreadable; key empty; key of 31 bytes; key of 32 bytes. A start-sequence check confirms that the composition root of `serve` (12) calls the validation before it binds the listener (the refused cases leave no listening socket).
**Expected result**: missing, unreadable, empty and 31-byte keys are refused, and the problem message names `share_grant_key` and never contains key bytes; the 32-byte key is accepted. E-01's secret loader checks only that the file exists and is readable, so the length rule is asserted here and not duplicated there.
**Max execution time**: 2 s
**Dependencies**: none (no database)

## Performance Tests

Architecture §6 has no sharing number; targets derive from AC-12 (opens in <= 2 s, constant-work lookup within 1 ms).

### PT-06-1: Open and content authorisation
**Summary**: per-request cost of `open` and `authorize_content`.
**Traces to**: AC-12
**Load scenario**:
- Concurrent users: 200 visitors on 1,000 links (20% password links with grants)
- Request rate: 1,000 req/s mix (50% authorize_content, 50% open)
- Duration: 3 minutes, ramp-up 20 s

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| `open` p95 | <= 10 ms | 50 ms |
| `authorize_content` p95 (cache miss, 2 PK reads) | <= 15 ms | 60 ms |
| Error rate | 0% | 0% |
**Resource limits**: DB connections <= 12. Env C-REF.

### PT-06-2: Timing uniformity of missing links
**Summary**: medians of unknown, expired, revoked and deleted-target lookups differ by <= 1 ms.
**Traces to**: AC-12
**Load scenario**: 10,000 samples per case, interleaved, single client, quiet host, `share_lookup_cache` as production.

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Median difference between any two cases | <= 0.3 ms | 1 ms |
| Response value inequality | 0 | 0 |
**Env**: C-REF (system: NFT-SEC-17).

### PT-06-3: Password hash-cost flood
**Summary**: wrong-password floods cannot starve other requests.
**Traces to**: AC-20
**Load scenario**: 1,000 concurrent `verify_password` with wrong passwords across 100 links for 60 s while 50 visitors call `open`.

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Argon2 concurrency (max permits in use) | <= `share_password_max_concurrency` | exceeded |
| `open` p95 during flood | <= 50 ms | 200 ms |
| CPU used by hashing | <= permits x 1 core | permits x 1.5 |
**Env**: C-REF (system: NFT-SEC-06).

## Security Tests

### ST-06-1: Cross-account owner operations
**Summary**: account B cannot create, list or revoke A's links.
**Traces to**: AC-59
**Attack vector**: A's node id and share id used by B.
**Test procedure**:
1. A creates a link on f. 2. B calls `create(B, f)`, `list(B)`, `list(B, node = f)`, `revoke(B, A.share_id)`. 3. A's link opened by a visitor.
**Expected behavior**: `TargetNotFound`, empty lists, `NotFound`; A's link still works.
**Pass criteria**: 0 successful cross-account calls; A's row unchanged. **Fail criteria**: any change or any listing of A's link.

### ST-06-2: Token strength and storage
**Summary**: tokens are 256-bit random with prefix, stored hashed, never logged.
**Traces to**: AC-12
**Attack vector**: database/log disclosure, token prediction.
**Test procedure**: create 10,000 links; decode tokens; dump `share_links` with the migrator role and capture logs and audit rows.
**Expected behavior**: prefix `sc_shr_`; >= 128 bits (actual 256) entropy, 10,000 distinct, no common prefix beyond the marker; DB holds only hashes; no token or password in logs/audit.
**Pass criteria**: 0 plaintext matches; chi-square on bytes not rejecting uniformity at p = 0.001. **Fail criteria**: any plaintext token found. System: NFT-SEC-16.

### ST-06-3: Enumeration
**Summary**: guessing tokens yields identical answers and trips the IP limit.
**Traces to**: AC-12, AC-20
**Attack vector**: 10,000 random and near-miss tokens (one char changed from a live token, truncated, case-changed).
**Test procedure**: call `open` from one IP.
**Expected behavior**: every answer is `NotFound` until call 101 per minute, then `RateLimited`; no near-miss differs from random.
**Pass criteria**: 0 hits, response values equal, limit trips at 101. **Fail criteria**: any distinguishable answer.

### ST-06-4: Grant forgery and replay
**Summary**: the HMAC grant cannot be forged, transplanted or extended.
**Traces to**: AC-12
**Attack vector**: change `expires_at`, grant of another link, grant after revoke, empty/oversized grant, comparison timing.
**Test procedure**: 1,000 mutated grants (bit flips at every byte offset) against L1 and L2.
**Expected behavior**: all `PasswordRequired`; constant-time compare (median timing of valid-prefix vs invalid within 1 ms).
**Pass criteria**: 0 accepted mutations. **Fail criteria**: any acceptance.

### ST-06-5: Password storage and DoS
**Summary**: Argon2id with configured parameters; verification bounded.
**Traces to**: AC-20
**Attack vector**: offline guess of a leaked hash; CPU exhaustion.
**Test procedure**: parse `password_hash` PHC strings; run PT-06-3 scale at 5 s.
**Expected behavior**: algorithm `argon2id` with configured m/t/p; unique salts; permits respected.
**Pass criteria**: 100% PHC strings argon2id, 0 duplicates salts. **Fail criteria**: otherwise.

### ST-06-6: Wrong-password limit cannot be bypassed
**Summary**: the 10-per-link limit holds across IPs, parallel calls and token casing.
**Traces to**: AC-20
**Attack vector**: 50 parallel wrong attempts from 50 IPs against one link; same link via trailing-slash/percent-encoded token forms (as received after 11 normalisation).
**Test procedure**: fire at a barrier; count verification attempts executed.
**Expected behavior**: at most 10 verifications executed per 10-minute window; the rest `RateLimited`.
**Pass criteria**: <= 10 Argon2 verifications. **Fail criteria**: more than 10.

### ST-06-7: Visitor disclosure
**Summary**: visitor results expose only name, size, type and expiry.
**Traces to**: AC-12
**Attack vector**: read `ShareOpen`, `ShareContent` and errors for owner, account id, path.
**Test procedure**: compile-time field list of `Available`; serialise all outcomes.
**Expected behavior**: no account id, owner, path or node id in `open`/`verify_password` results; `ShareContent` (account id, node id, revision id) is internal to 11/03 and never serialised to the visitor (asserted in 11's tests).
**Pass criteria**: field list equals the four allowed fields. **Fail criteria**: any extra identifying field.

### ST-06-8: RLS and the single cross-account lookup
**Summary**: only the token lookup runs in system scope.
**Traces to**: AC-59
**Attack vector**: scoped query on `share_links`; enumeration through the lookup function.
**Test procedure**: as role `sunnycloud` scoped to B: `SELECT * FROM share_links`; call the lookup function with a token hash; inspect which functions switch scope.
**Expected behavior**: B sees only B rows; the lookup returns one row by exact `token_hash` and nothing for partial input.
**Pass criteria**: 0 foreign rows from scoped queries; lookup not usable as a scan. **Fail criteria**: otherwise.

## Acceptance Tests

### AT-06-1: Share a photo and open it
**Summary**: owner creates a link; a visitor opens it.
**Traces to**: AC-12, AC-47
**Preconditions**: account with a JPEG.
**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `create` default | token shown once, `expires_at` +7 d |
| 2 | visitor `open` | `Available{name, size, image/jpeg}` |
| 3 | `authorize_content(starts_download = true)` | current revision; one `share_accessed` event |
| 4 | `revoke`, then `open` | `NotFound` within the cache window |

### AT-06-2: Password-protected link
**Summary**: password flow end to end.
**Traces to**: AC-12, AC-20
**Preconditions**: link with a password.
**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `open` | `PasswordRequired` |
| 2 | 10 wrong passwords, then the 11th | `WrongPassword` x10, then `RateLimited{retry_after}` |
| 3 | after the window, right password, `authorize_content` | grant, then `ShareContent` |

### AT-06-3: Link survives edits and moves, dies with delete
**Summary**: owner-visible link behaviour over the file's life.
**Traces to**: AC-34, AC-77
**Preconditions**: link on `notes.txt`.
**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | save-in-place edit | same link, new `revision_id` |
| 2 | rename and move | same link works |
| 3 | delete the file | `NotFound` within 5 s |

## Test Data Management

**Required test data**:

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-17 (S) | hostile and positive media types (media type decision input) | generated | < 5 MB |
| Link factory | creates N links with random TTL/password across accounts | code, seeded | 10,000 rows |
| Password corpus | ASCII, Unicode, 1 kB passwords | static file | < 50 KB |
| Token corpus | random and near-miss tokens | seeded generator | < 2 MB |

**Setup procedure**:
1. One `postgres:18@sha256:<pin>` per binary; migrate as `sunnycloud_migrator` into `tmpl`; per test `CREATE DATABASE ... TEMPLATE tmpl`; component runs as `sunnycloud`, fixtures as `sunnycloud_migrator`.
2. Accounts, files and revisions created through real 01/02/03 factories; manual `Clock`; `share_grant_key` test secret; ms-scale TTLs and windows.

**Teardown procedure**: drop the database; reset limiter state (new `AbuseLimiter` per test); connection baseline check.

**Data isolation strategy**: database per test from a migrated template; the abuse limiter and grant key are per-test instances so counters never leak between tests; generated corpora are read-only and shared.
