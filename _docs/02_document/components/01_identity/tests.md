# Test Specification — 01_identity

Tier: component integration (Rust, `cargo test -p identity`), runs at the public port boundary of the crate (`Authenticator`, `SignInService`, `SessionService`, `DeviceGrantService`, `InviteService`, `OperatorIdentityOps`, `AccountAdmin`, `AccountGuard`, `PrincipalRevalidator`, `Janitor`). Env class `C` (container: Postgres only, no Compose stack); timing rows marked C-REF count only on REF-SRV. System-level scenarios are referenced by ID and not restated (`tests/blackbox-tests.md`, `security-tests.md`, `resilience-tests.md`).

External dependency doubles: the OIDC/GitHub provider is the scripted `mock-idp` behind the `ProviderClient` trait (external service, allowed). `GeoLookup` is a fixed-table implementation. `Clock` is the manual test clock (helper 03). Nothing that stores data is faked: every repository runs on real PostgreSQL 18.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion (short) | Test IDs | Coverage |
|-------|------------------------------|----------|----------|
| AC-19 | Sign in with each configured provider; operator picks subset | IT-01-4, IT-01-5, AT-01-1 | Covered here + FT-P-62, FT-N-25 (server start without provider is helper 01/12 config validation) |
| AC-20 | Abuse limits for sign-in, device approval | IT-01-13 | Covered here (counters) + NFT-SEC-04, NFT-SEC-05 (per-IP over HTTP) |
| AC-21 | Revoked session/device stops within 5 s, incl. live streams | IT-01-1, IT-01-2, IT-01-15, PT-01-1 | Covered here (authn half) + NFT-SEC-08, FT-N-30 (held stream end to end, 05 + 11) |
| AC-23 | Setup token once; later registration by invite only | IT-01-7, IT-01-9, IT-01-14, AT-01-1 | Covered here + FT-P-64, FT-P-65, FT-N-20, FT-N-21, NFT-SEC-01 |
| AC-54 | Server side: https public URL | IT-01-4 | Covered here (redirect_uri from `public_url`) + FT-N-26, NFT-SEC-12 (apps) |
| AC-55 | Unverified provider email never creates or links | IT-01-6 | Covered here + FT-N-22, NFT-SEC-02 |
| AC-56 | 12 negative OAuth callbacks | IT-01-5 | Covered here (all 12 at service level) + FT-N-23, NFT-SEC-03 |
| AC-57 | Device approval shows IPs/locations; code expires; device tokens refused on session-only scopes | IT-01-3, IT-01-12 | Covered here + FT-P-66, FT-N-24, NFT-SEC-07 |
| AC-58 | Refresh rotation: 4 concurrent give 1 successor; replay revokes family | IT-01-10 | Covered here (rotation part; CSRF part is 11) + NFT-SEC-10 |
| AC-59 | Two-account matrix (authn half) | ST-01-1, ST-01-2 | Covered here + NFT-SEC-13, NFT-SEC-14 |
| AC-75 | Two concurrent redemptions of one invite create one account | IT-01-8, IT-01-17 | Covered here + NFT-SEC-48 |
| AC-76 | Device code exchanged exactly once, never after expiry | IT-01-11 | Covered here + NFT-SEC-49 |
| AC-79 | Worker endpoints reject user/device tokens | IT-01-3, ST-01-4 | Covered here (no worker credential/scope exists in 01) + NFT-SEC-50 (listener, 08 + 11) |
| AC-27 | Account deletion removes sessions/identities (finalize) | IT-01-15 | Covered here (01 half) + FT-P-19, NFT-RES-30, NFT-SEC-27 |
| AC-68..AC-74, AC-77, AC-80 | Interaction ACs not owned by 01 | IT-01-16 (AccountGuard seam for AC-68/AC-74) | Covered by system scenarios NFT-RES-34, NFT-RES-38 (seam test IT-01-16 here) |

## Blackbox Tests

### IT-01-1: One uniform Unauthenticated for every dead credential
**Summary**: unknown, expired, revoked, disabled-account and deleting-account credentials all fail `authenticate` with the same variant through the same single indexed probe.
**Traces to**: AC-21, AC-59
**Description**: create account A with one session S and one device token D (via invite and device grant). Prepare 8 credentials: random never-issued; S expired (clock +access_token_ttl+refresh_token_ttl); S revoked; D revoked; S of a `disabled` account; S of a `deleting` account (`begin_deletion`); malformed prefix; empty string.
**Input data**: `Credential::Session(token)` / `Credential::Device(token)` for each case; config `session_cache_ttl = 0ms` for this test.
**Expected result**:
```
Each of the 8 calls -> Err(Unauthenticated) (same enum value, no payload difference)
Valid S and D before revocation -> Ok(Principal{account_id: A, kind, role, label})
Captured SQL log per failing call: exactly 1 statement on a *_hash unique index, 0 writes
```
**Max execution time**: 5 s; **Dependencies**: PostgreSQL 18 container

### IT-01-2: Revocation visible within the cache TTL
**Summary**: `revalidate` turns false no later than `session_cache_ttl` after `revoke`, `revoke_all`, `revoke_device`, `set_state(disabled)`.
**Traces to**: AC-21
**Description**: with `session_cache_ttl = 200ms`, authenticate (warms cache), call each revocation, poll `revalidate(&principal)` every 20 ms.
**Input data**: 4 principals (2 session, 2 device) each revoked by a different path.
**Expected result**:
```
revalidate == true before revocation; flips to false within <= 200 ms + 50 ms margin for all 4
authenticate after flip -> Unauthenticated
```
**Max execution time**: 3 s; **Dependencies**: PostgreSQL

### IT-01-3: Scope matrix, no worker scope
**Summary**: `require` enforces `Public / SessionOnly / SessionOrDevice / Admin` for session member, session admin and device principals; no `Worker` credential exists.
**Traces to**: AC-57, AC-79
**Description**: build principals {member session, admin session, device token}; call `require` for every `Scope`.
**Input data**: 3 principals x 4 scopes = 12 calls, plus a worker-style bearer string (`wk_...`) passed as `Credential::Session` and as `Credential::Device`.
**Expected result**:
```
member: Public ok, SessionOnly ok, SessionOrDevice ok, Admin Forbidden
admin session: all ok
device: Public ok, SessionOnly Forbidden, SessionOrDevice ok, Admin Forbidden
worker-style bearer in either variant -> Unauthenticated
Credential enum has exactly two variants (compile-time exhaustive match test)
```
**Max execution time**: 2 s; **Dependencies**: PostgreSQL

### IT-01-4: begin_sign_in validation
**Summary**: `begin` rejects bad providers, return paths and hosts and builds `redirect_uri` from `public_url`, never from the host.
**Traces to**: AC-19, AC-54, AC-56
**Description**: call `begin` with the inputs below against `public_url = https://sunnycloud.test`.
**Input data**: provider `nosuch`; `return_to` in `{"/maps", "https://evil.test/", "//evil.test", "/\\evil.test", "/x\r\nSet-Cookie: a=b", ""}`; host header `evil.test`; mobile `client_kind` without `code_challenge`; GitHub and OIDC providers valid.
**Expected result**:
```
nosuch -> UnknownProvider; each hostile return_to -> InvalidReturnPath; host evil.test -> HostMismatch
mobile without code_challenge -> error (invalid request); valid -> AuthorizationRedirect{url, browser_binding}
url starts with the provider authorize endpoint and contains redirect_uri=https%3A%2F%2Fsunnycloud.test%2Fapi%2Fv1%2Fauth%2Fproviders%2F{key}%2Fcallback
oauth_states row written with state_hash = SHA-256(state || browser_binding); plaintext state absent from DB
public_url with http:// and allow_plain_http_dev=false -> service construction fails
```
**Max execution time**: 3 s; **Dependencies**: PostgreSQL, mock-idp

### IT-01-5: The 12 negative callbacks at service level
**Summary**: each of the 12 AC-56 cases ends in the documented error, creates no session, no account and consumes nothing it should not.
**Traces to**: AC-56, AC-19
**Description**: run `begin` then `complete` with one mutation per case.
**Input data**:
```
1 missing state            -> UntrustedState
2 replayed state (2nd complete) -> UntrustedState
3 foreign state (other browser_binding) -> UntrustedState
4 expired state (clock + oauth_state_ttl) -> UntrustedState
5 wrong provider key in callback path -> UntrustedState
6 wrong nonce in id_token -> TrustedFailure{code}
7 wrong audience -> TrustedFailure
8 wrong issuer -> TrustedFailure
9 absolute return_to at begin -> InvalidReturnPath
10 foreign Host at complete -> HostMismatch
11 reused one-time provider code -> TrustedFailure
12 wrong code_verifier at exchange_mobile_code -> InvalidCode
```
**Expected result**:
```
Each case: no row in sessions/accounts/identities/login_codes beyond the fixture; DB row counts before == after
Cases 1-5 consume no state row other than their own; case 2 second call finds no row
```
**Max execution time**: 10 s; **Dependencies**: PostgreSQL, mock-idp scenarios (same corpus as DS-20)

### IT-01-6: Email is read only at invite redemption and only when verified
**Summary**: unverified or absent email never creates or links an account.
**Traces to**: AC-55
**Description**: mock returns victim email `victim@example.test` with `email_verified=false` (OIDC), GitHub per-address `verified=false`, Apple-style relay address; allowlist contains `victim@example.test`. Then the same with `email_verified=true`.
**Input data**: 3 unverified variants, 1 verified; existing account for the victim; invite token absent in the first run.
**Expected result**:
```
Unverified: complete -> InvitationRequired; accounts count unchanged; identities count unchanged; begin_link/complete_link links nothing
Verified + allowlisted but no invite -> InvitationRequired (email never matches an account by itself)
Verified + valid invite -> account created; email not stored as login key
```
**Max execution time**: 5 s; **Dependencies**: PostgreSQL, mock-idp

### IT-01-7: Setup token bootstrap
**Summary**: on an empty database a social login without the token creates nothing; the token works once.
**Traces to**: AC-23
**Description**: empty DB; `ensure_setup_token()`; `complete` without, with wrong, with correct, then with the same token again; then `ensure_setup_token()` after an account exists.
**Input data**: setup token plaintext returned once; clock +`setup_token_ttl` run.
**Expected result**:
```
without/wrong token -> InvitationRequired, 0 accounts
correct -> 1 account with role admin; token row redeemed
second use -> InvitationRequired; accounts still 1
ensure_setup_token with accounts present -> None
ensure_setup_token called twice while empty -> second call replaces the open token; first plaintext no longer works
after setup_token_ttl -> InvitationRequired
```
**Max execution time**: 5 s; **Dependencies**: PostgreSQL, mock-idp

### IT-01-8: Concurrent invite redemption creates one account
**Summary**: N parallel `complete` calls with one invite token create exactly one account.
**Traces to**: AC-75
**Description**: 200 iterations; each issues one member invite and runs 2, then 16, parallel `complete` calls from distinct provider identities on separate pool connections released by a barrier.
**Input data**: invite from `create_invite`; 16 distinct mock identities (verified emails).
**Expected result**:
```
Per iteration: accounts +1, identities +1, invites.redeemed_at set once; exactly 1 call Ok(Session), all others Err(InvitationRequired)
Losers leave no accounts, identities, root node, feed or quota rows (hooks' initialize rolled back with them)
0 iterations with 2 accounts across 200 iterations
```
**Max execution time**: 60 s; **Dependencies**: PostgreSQL; registered lifecycle hooks 02, 05, 07a (real implementations)

### IT-01-9: Operator invites and admin re-bind
**Summary**: `create_admin_invite` makes an admin; `reset_admin` re-binds the existing account without creating a new one.
**Traces to**: AC-23
**Description**: admin X with a session and a node tree; `reset_admin(X)`; a new identity redeems the returned invite.
**Input data**: `OperatorIdentityOps::reset_admin(X)`; mock identity Y.
**Expected result**:
```
X's sessions revoked immediately; invite has target_account_id = X, created_by NULL
Redemption: accounts count unchanged; identities of X = {Y} only (old removed); role admin kept; X's root node and files untouched
Redeeming the same invite again -> InvitationRequired
```
**Max execution time**: 5 s; **Dependencies**: PostgreSQL, real 02 root node hook

### IT-01-10: Refresh rotation under concurrency and replay
**Summary**: four concurrent refreshes with one token give one successor and no revocation; a replay after the grace window revokes the family.
**Traces to**: AC-58
**Description**: `refresh_reuse_grace = 300ms`. Fire 4 parallel `refresh(token_0)`. Then wait 400 ms (clock advance) and call `refresh(token_0)` again.
**Input data**: one session with refresh token token_0.
**Expected result**:
```
After the 4 parallel calls: exactly 1 new refresh token row in the family, family not revoked, access of the old session still valid until its ttl
Replay after grace -> ReuseDetected; every session/token of the family revoked within 5 s (poll authenticate) with reason refresh_reuse; one audit event; one WARN log with ids only
```
**Max execution time**: 5 s; **Dependencies**: PostgreSQL

### IT-01-11: Device code exchanges exactly once and never after expiry
**Summary**: `poll` mints a device token once under concurrency and not after `device_code_ttl`.
**Traces to**: AC-76
**Description**: `request` -> approve with a session principal -> 20 parallel `poll`. Second case: approve at t0, advance the clock beyond `device_code_ttl` (test 2 s), then `poll`. Third: `poll` faster than `device_poll_interval`; deny path.
**Input data**: device name `lab-1`, platform linux, requester IP 10.0.0.5.
**Expected result**:
```
Parallel: exactly 1 Ok(DeviceToken); the other 19 error (ExpiredToken/AccessDenied family, never a token); device_tokens +1; grant status consumed
Sequential second poll -> error, no new token
Expired-after-approve -> ExpiredToken, device_tokens +0
Too fast -> SlowDown; denied -> AccessDenied; AuthorizationPending before decision
```
**Max execution time**: 15 s; **Dependencies**: PostgreSQL

### IT-01-12: Approval view shows requester vs approver
**Summary**: `lookup_for_approval` returns both IPs and locations and flags a mismatch; user code expires.
**Traces to**: AC-57
**Description**: requester IP in city X, approver IP in city Y (fixed `GeoLookup` table); same-IP case; missing GeoIP file (`GeoLookup` returns unknown).
**Input data**: IP pairs 203.0.113.7/198.51.100.9, 203.0.113.7/203.0.113.7; clock +`device_code_ttl`.
**Expected result**:
```
Different cities: mismatch = true with both locations; same IP: mismatch = false
GeoLookup unknown + different IPs: location unknown, mismatch = true
After ttl: lookup_for_approval -> UnknownCode (counted) ; decide -> Expired; second decide -> AlreadyDecided
```
**Max execution time**: 3 s; **Dependencies**: PostgreSQL

### IT-01-13: Device-approval and sign-in counters
**Summary**: the AC-20 limits trip at the stated counts and return `RateLimited{retry_after}`.
**Traces to**: AC-20
**Description**: with the real `AbuseLimiter` (windows scaled to ms in config): 5 wrong user codes by one approving session; 20 by one account across sessions; 50 by one IP across accounts; 10 failed sign-in steps from one IP.
**Input data**: wrong `user_code` values; limits from AC-20 (5 / 20 / 50 / 10).
**Expected result**:
```
Attempts 1..N-th return UnknownCode; the (N+1)-th returns RateLimited with retry_after > 0 for each dimension
A correct code still approved by a different approving session of another account
After the window (clock) the counter resets; each trip writes one audit event
```
**Max execution time**: 8 s; **Dependencies**: PostgreSQL, rate-limit helper 08

### IT-01-14: Mobile one-time code exchange and IdentityMaintenance janitor
**Summary**: `exchange_mobile_code` is single-use, PKCE-bound and expires (result is a `sessions` row); `run_cycle(now)` removes only expired `oauth_states`, `login_codes`, grants (1 h after expiry) and sessions (30 d after revoke/expiry), twice with the second run deleting 0.
**Traces to**: AC-19, AC-58
**Description**: complete a mobile sign-in (`client_kind = android`) to get `MobileCode`; exchange with right verifier; again; wrong verifier on a fresh code; expired code.
**Input data**: `code_verifier` with SHA-256 challenge; `login_code_ttl` = 500 ms.
**Expected result**:
```
First exchange -> SessionTokens (access ttl 15 min config, rotating refresh), sessions.client_kind = android
Second exchange, wrong verifier, expired -> InvalidCode (same value); login_codes row consumed by DELETE ... RETURNING; no session minted for failures
```
**Max execution time**: 4 s; **Dependencies**: PostgreSQL, mock-idp

### IT-01-15: AccountAdmin and lifecycle
**Summary**: `set_state`, `begin_deletion`, `finalize_deletion`, `lock_active_admins`, `revoke_self`, `list_devices` behave per contract.
**Traces to**: AC-21, AC-27
**Description**: account with 2 sessions, 2 device tokens, 1 open grant; apply each admin call inside a caller `DbTx`.
**Input data**: `set_state(disabled)`, `begin_deletion`, then (with all hooks reporting 0) `finalize_deletion`; `revoke_self` by device principal and by session principal.
**Expected result**:
```
disabled/deleting: authenticate -> Unauthenticated; sessions, device tokens, grants revoked in the same transaction (rollback of the caller tx undoes it)
lock_active_admins returns active admin ids and a second transaction's call blocks until commit (pg_locks)
finalize_deletion leaves 0 rows in identities, sessions, device_tokens, device_grants, login_codes, accounts for the id
revoke_self by a device revokes only its own row; by a session principal -> Forbidden (scope)
```
**Max execution time**: 8 s; **Dependencies**: PostgreSQL

### IT-01-16: AccountGuard against begin_deletion
**Summary**: `ensure_active` (FOR SHARE) lets an in-flight commit finish before `begin_deletion` (FOR UPDATE), and refuses later commits.
**Traces to**: AC-74, AC-68 (seam)
**Description**: connection 1 opens a transaction and calls `ensure_active`; connection 2 calls `begin_deletion` and blocks; connection 1 commits; then connection 3 calls `ensure_active`.
**Input data**: one active account.
**Expected result**:
```
begin_deletion visible in pg_locks as waiting until connection 1 commits, then completes (wait > 0, < 2 s)
connection 3 -> Err(AccountNotActive)
```
**Max execution time**: 5 s; **Dependencies**: PostgreSQL

### IT-01-17: Account creation is one transaction over the hooks
**Summary**: a failing `initialize` hook leaves no account, identity or redeemed invite.
**Traces to**: AC-75
**Description**: register hooks 02, 05, 07a real, plus a fault-injecting hook (test double of the `AccountLifecycleHook` port) failing on its first call; redeem an invite.
**Input data**: invite; injected hook error.
**Expected result**:
```
complete -> typed failure; accounts/identities/root node/account_feeds/quota unchanged; invite still redeemable; second attempt without the fault succeeds with all four rows created in one transaction
```
**Max execution time**: 5 s; **Dependencies**: PostgreSQL, 02/05/07a hooks. Hooks are handed over through `register_hooks` (01 description, AccountLifecycleHook; decided 2026-11-11, AZ-3681). A last step creates an account on a fresh service where `register_hooks` was never called: typed failure, nothing written, invite still redeemable.

## Performance Tests

Architecture §6 gives no authn number; targets below are derived from AC-1 (100 concurrent sessions, 500 users) and AC-21 (5 s revocation) and are marked "derived".

### PT-01-1: authenticate hot path
**Summary**: cache-hit and cache-miss `authenticate` cost.
**Traces to**: AC-1, AC-21
**Load scenario**: 100 principals on 16 tasks, 5,000 req/s authenticate (95% repeat tokens, 5% fresh), 2 minutes, ramp-up 10 s.
**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| Latency p95, cache hit | <= 0.2 ms | 1 ms |
| Latency p95, cache miss | <= 3 ms | 8 ms |
| Latency p99, miss | <= 6 ms | 15 ms |
| Error rate | 0% | 0% |
**Resource limits**: CPU <= 1 core for the service tasks; DB connections <= 8; memory growth <= 20 MB.
**Env**: C-REF

## Security Tests

### ST-01-1: Cross-account administration (IDOR)
**Summary**: account B cannot see or revoke account A's sessions and devices.
**Traces to**: AC-59
**Attack vector**: B passes A's session id / device id to `SessionService::revoke`, `list`, `DeviceGrantService::revoke_device`, `list_devices`, `decide` on A's grant.
**Test procedure**: create accounts A and B with 2 sessions and 2 devices each; as B call every method above with A's ids; re-authenticate A's credentials.
**Expected behavior**: id-bearing calls return `NotFound` (or no row changed for `revoke`); lists contain only B's rows.
**Pass criteria**: 0 successful cross-account calls; A's sessions still valid; no audit event names A as actor.
**Fail criteria**: any A row changed or listed.

### ST-01-2: Row-level security with the runtime role
**Summary**: connected as `sunnycloud` (no `BYPASSRLS`), a query scoped to account B never returns A's rows.
**Traces to**: AC-59
**Attack vector**: raw `SELECT` on `sessions`, `device_tokens`, `identities` under account scope B; attempt `SET ROLE sunnycloud_migrator`, `CREATE TABLE`, `ALTER`.
**Test procedure**: fixture rows for A and B via the migrator role; run the selects with the API pool scoped to B; attempt DDL and role switch.
**Expected behavior**: only B rows visible; DDL and `SET ROLE` denied.
**Pass criteria**: 0 foreign rows; DDL fails with permission denied. (System level: NFT-SEC-46.)
**Fail criteria**: any foreign row or successful DDL.

### ST-01-4: Worker or foreign bearer on user credentials
**Summary**: no credential outside session/device opens a principal (AC-79 identity half).
**Traces to**: AC-79
**Attack vector**: present the worker bearer value, a refresh token as access token, a `login_code`, and a device_code as `Credential`.
**Test procedure**: call `authenticate` with each in both variants.
**Expected behavior**: `Unauthenticated`.
**Pass criteria**: 4/4 rejected; no row touched. **Fail criteria**: any principal returned.

### ST-01-5: Token storage and entropy
**Summary**: only SHA-256 hashes of 256-bit typed tokens are stored; nothing secret is logged.
**Traces to**: AC-21, AC-58
**Attack vector**: database or log disclosure.
**Test procedure**: mint 100 session, refresh, device, invite, login-code and setup tokens; dump all columns of owned tables with the migrator role; capture logs and audit `detail`.
**Expected behavior**: no plaintext token substring in any column, log or audit row; tokens have a type prefix and decode to >= 32 random bytes; 100 tokens pairwise distinct.
**Pass criteria**: 0 matches. **Fail criteria**: any match.

## Acceptance Tests

### AT-01-1: First admin, then member by invite
**Summary**: an operator bootstraps and onboards a user using only the ports.
**Traces to**: AC-19, AC-23, AC-75
**Preconditions**: empty database; mock-idp with two identities.
**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `ensure_setup_token`, identity 1 signs in with it | admin account, session |
| 2 | admin `create_invite` (member), identity 2 signs in with it | member account, role member |
| 3 | identity 3 signs in without invite | `InvitationRequired`, accounts = 2 |

## Test Data Management

**Required test data**:

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-20 | mock-idp scenarios (valid, 12 negatives, unverified email, slow provider) | `tests/mocks/idp/scenarios/*.yaml` | < 1 MB |
| DS-32 | GeoIP test table for two cities | fixture | < 100 KB |
| Account factory and a failing `AccountLifecycleHook` double | creates accounts through invites and the real `initialize` hooks; one hook errors once | code | n/a |

**Setup procedure**: (1) start one `postgres:18@sha256:<pin>` container per test binary (testcontainers); run `sunnycloud migrate` as `sunnycloud_migrator` into database `tmpl`; (2) per test: `CREATE DATABASE t_<uuid> TEMPLATE tmpl`; the component under test connects as role `sunnycloud` (RLS on, no DDL); fixtures and probes connect as `sunnycloud_migrator`; (3) inject the manual `Clock`; durations (grace, ttl, windows) are millisecond config values.

**Teardown procedure**: drop the per-test database; stop the container at the end of the binary; leak check: connection count back to baseline.

**Data isolation strategy**: one database per test from a migrated template (no shared mutable state, safe for concurrency tests that need several connections); transactions are not used as the isolation device. Read-only fixtures (DS-20, DS-32) are shared; expensive container is shared across tests of the binary.
