# 01_identity — Identity, sessions, device grants, invites

## 1. High-Level Overview

**Purpose**: Decide who a caller is: sign-in through GitHub / generic OIDC (Apple v1.1), server-owned sessions with refresh rotation, desktop device grant, mobile one-time-code hand-off, single-use invites and the setup token, and the scope check every other component relies on.

**Single responsibility**: authentication and credential lifecycle. It never decides what a caller may see in the tree (callers are scoped by `account_id` elsewhere) and depends on no other component crate: every cross-component call is a port trait in `contracts` (see Ports).

**Architectural pattern**: service traits over repositories; a pluggable `ProviderClient` per provider kind; a lifecycle-hook trait so account creation can span components without identity knowing them; an account guard so commits cannot race account deletion.

**Scope in**: providers and OAuth/OIDC state, accounts and identities, sessions and refresh families, device tokens and device grants, invites and the setup token, the account guard, scope model, per-failure abuse limits (via the rate-limit helper), audit events for sign-in/device/session.
**Scope out**: CSRF/Origin checks, cookie attributes, 429 mapping (11_http_api); invite/user admin screens (07_admin calls the ports below); account deletion orchestration and the erase of quota, feed, tree, transfer, share and media rows (07_admin drives the `AccountLifecycleHook`s implemented there); worker authentication (08 `WorkerAuthenticator`, internal listener only).

**Upstream dependencies**: helpers only (config, error, clock, audit, ids_hashing, db, ratelimit), `contracts`, outbound HTTPS to providers.
**Downstream consumers**: through ports only; see Ports.

## 2. Internal Interfaces

All methods are async; DB-touching methods take the unit-of-work handle `DbTx` (helper 07) where they must join a caller transaction, otherwise they open their own short transaction. No HTTP types in signatures. The traits below are the ports named in the Ports section.

### Authenticator (called by 11 middleware, 05)
| Method | Input | Output | Errors |
|---|---|---|---|
| `authenticate` | `Credential{Session(token)\|Device(token)}` | `Principal{account_id, kind, session_or_device_id, role, label}` | `Unauthenticated` (missing, unknown, expired, revoked: one variant, same DB path) |
| `revalidate` | `&Principal` | `bool` | none (cache at most `session_cache_ttl`) |
| `require` | `&Principal, Scope` | `Result<(), Forbidden>` | `Forbidden` |
`Scope` = `Public`, `SessionOnly` (approve-device, create-share, delete-account, link-provider, device/session management), `SessionOrDevice` (tree, transfer, feed, quota read), `Admin` (session with role admin). `DELETE /devices/me` (`revoke_self`) is device-only (owner decision 2026-11-12): 11 declares it `DeviceOnly` and `revoke_self` refuses a session principal, so a browser removes a device through `DELETE /devices/{id}`. Worker routes on the internal listener are not in this scope model: 08 `WorkerAuthenticator` owns them and a user or device token never opens them (AC-79). Every OpenAPI operation of the public listener declares one scope and CI fails on an operation without one.

### SignInService
| Method | Input | Output | Errors |
|---|---|---|---|
| `begin` | provider key, `return_to` (relative), `client_kind`, optional `invite_token`/`setup_token`, `code_challenge` (required for mobile `client_kind`), public-host check | `AuthorizationRedirect{url, browser_binding}` | `UnknownProvider`, `InvalidReturnPath`, `HostMismatch`, `RateLimited{retry_after}` |
| `complete` | provider key, callback params, `browser_binding`, host | `SignInOutcome::Session{tokens, return_to}` \| `MobileCode{code, return_to}` (the code row copies `code_challenge` from the state row) | `UntrustedState` (unknown/foreign/replayed/expired: 400), `TrustedFailure{code}` (redirect to SPA login with code), `InvitationRequired`, `ProviderUnavailable` (10 s), `RateLimited` |
| `begin_link` / `complete_link` | session principal, provider | redirect / `Linked` | as above plus `IdentityTaken` |
| `exchange_mobile_code` | `code`, `code_verifier` | `SessionTokens` | `InvalidCode` (single use, PKCE mismatch, expired) |
| `list_providers` | none | `[ProviderInfo]`, `setup_required` | none |
State row (`oauth_states` gets a nullable `code_challenge` column for mobile sign-in; the data model carries it): `state_hash = SHA-256(state ‖ browser_binding)`, so a state without the matching cookie is simply not found (case 3) without a schema change. Consumed by `DELETE … RETURNING`. Provider calls: 10 s timeout, no redirects, link-local/metadata ranges blocked, operator-configured issuers only.

### SessionService
`refresh(refresh_token) -> SessionTokens | ReuseDetected | Unauthenticated`; `revoke(account, session_id, reason)`; `revoke_all(account, reason)`; `list(account) -> [SessionInfo]`; `touch` (last_seen at most once a minute). Rotation per data model 4.3: four concurrent refreshes yield one successor; replay after `refresh_reuse_grace` revokes the family with reason `refresh_reuse` and audits it.

### DeviceGrantService (RFC 8628, desktop only)
| Method | Input | Output | Errors |
|---|---|---|---|
| `request` | device name, platform, requester IP | `DeviceAuthorization{device_code, user_code, interval, expires_in}` | none |
| `poll` | `device_code` | `DeviceToken` (once) | `AuthorizationPending`, `SlowDown`, `AccessDenied`, `ExpiredToken` |
| `lookup_for_approval` | session principal, `user_code`, approver IP | `ApprovalView{device, requester_ip, requester_location, approver_ip, approver_location, mismatch}` | `UnknownCode` (counted), `RateLimited` |
| `decide` | session principal, grant id, `Approve\|Deny` | `()` | `UnknownCode`, `Expired`, `AlreadyDecided` |
| `list_devices` / `revoke_device` | account / device id | `[DeviceInfo]` / `()` | `NotFound` |
| `revoke_self` | device principal | `()` | none (`DELETE /api/v1/devices/me`; a device token revokes only its own row) |
Token mint: `approved -> consumed` and the `device_tokens` insert are one `UPDATE … WHERE status='approved' AND expires_at > $now RETURNING` plus insert in one transaction, so a code exchanges exactly once and never after expiry (AC-76). Location comes from a local GeoIP file behind a `GeoLookup` trait (missing file: location unknown, mismatch warning still shown if IPs differ).

### InviteService / Operator operations
`create_invite(admin_account, note) -> InviteToken(plaintext once)` (member invite), `list`, `revoke` (admin, via 11). `invites` carries `grants_role` (`member` or `admin`) and `target_account_id` (NULL unless re-binding); `created_by` may be NULL for operator invites and the setup token (the data model relaxes its CHECK). `OperatorIdentityOps` (system scope, used by 12): `ensure_setup_token() -> Option<Plaintext>` (only while zero accounts; replaces the open one), `create_admin_invite` (`grants_role = admin`, no creator), `reset_admin(account)` (revokes that admin's sessions and issues an operator invite with `target_account_id` = that account; allowed for the only active admin too, owner decision 2026-11-11, G-12-3: the account keeps role admin and state active, so no `LastAdmin` rule applies and there is no refusal error; until the invite is redeemed nobody can sign in as that admin, and the CLI is the recovery path). Redeeming an invite with `target_account_id` creates no account: in the same transaction the signing-in identity is bound to the existing account and its previous identities are removed (the admin keeps the account, nodes and role). Admin promote and demote (`admin promote`, `admin demote`) go through 07 `AdminService`, which owns the last-admin rule (decision 15); identity has no `LastAdmin` error. Redemption is `UPDATE … WHERE redeemed_at IS NULL AND expires_at > $now RETURNING` inside the account-creation transaction: two concurrent redemptions create exactly one account (AC-75); the loser gets `InvitationRequired`.

### AccountAdmin (called by 07 `AdminService` and `DeletionPurger`)
Methods that change account rows take the caller's `&mut DbTx`, so 07 can combine them with its own checks in one transaction. 07 decides policy; identity only applies the change. There is no `update_user`: 07 calls `set_role` and `set_state` itself, after `lock_active_admins`, in one transaction.
| Method | Input | Output | Notes |
|---|---|---|---|
| `list_accounts` | filter, paging | `[AccountInfo]` | own short transaction |
| `lock_active_admins` | `&mut DbTx` | `Vec<AccountId>` | `SELECT … FOR UPDATE` on active admins; used by 07 for the last-admin check |
| `set_role` | `&mut DbTx`, account, `Role` | `()` | |
| `set_state` | `&mut DbTx`, account, `active\|disabled` | `()` | a disabled account fails `authenticate`; its sessions and device tokens are revoked in the same transaction |
| `begin_deletion` | `&mut DbTx`, account | `()` | takes the account row `FOR UPDATE`, state `deleting`, revokes sessions, device tokens and grants; `authenticate` and `AccountGuard` fail from now on |
| `finalize_deletion` | `&mut DbTx`, account | `()` | called by 07 only after every hook reports `remaining = 0`; deletes `identities`, `sessions`, `device_tokens`, `device_grants`, `login_codes` and the account row |

### AccountGuard
`ensure_active(&mut DbTx, account) -> Result<(), AccountNotActive>`: `SELECT state FROM accounts WHERE id = $1 FOR SHARE`. 02's `publish` calls it first inside the commit transaction, so a commit in flight finishes before `begin_deletion` (which takes the row `FOR UPDATE`) and any later commit is refused.

### AccountLifecycleHook (port in `contracts`, implemented by 02, 03, 05, 06, 07a, 08, 20)
`initialize(tx, account_id)` and `erase_batch(account_id, limit) -> EraseProgress{remaining}`. Account creation runs one transaction: `accounts`, `identities`, then every registered hook's `initialize` (05 creates `account_feeds` with the installation epoch it chooses itself, 07a creates `quota`, 02 creates the root node), then invite redemption. Identity calls only `initialize`. `erase_batch` is driven by 07's `DeletionPurger` until all hooks report zero, then 07 calls `finalize_deletion`; identity never calls `erase_batch` and imports no implementer. The hook list is registered late by 12 (decided 2026-11-11, AZ-3681): the identity service has an inherent method `register_hooks(&self, hooks: Vec<Arc<dyn AccountLifecycleHook>>) -> Result<(), HooksAlreadyRegistered>` (not a port in `contracts`: 12 builds the concrete service). It exists because 02, 05 and 07a take identity's `AccountGuard` or `PrincipalRevalidator` when they are built, while identity needs their hooks, so no constructor order works (07's `DeletionPurger` can take its list as a constructor parameter, G-07-1, because nothing it drives depends on it). The list is set once; a second call is `HooksAlreadyRegistered`, so a mis-wired composition root fails at start. 12 is the only caller and calls it before the API listeners accept requests. If account creation runs while no list was set, it fails closed: a typed failure (500 at the door, logged as `account_hooks_not_registered`), nothing written, the invite stays redeemable; a missing list is never read as an empty one, so no account exists without its quota row, feed and root node. Sign-in to an existing account and a re-bind redemption call no hook and are unaffected.

## Ports (crate `contracts`)
| Direction | Port | Counterpart | Used for |
|---|---|---|---|
| Provides | `Authenticator` (`authenticate`, `require`) | 11 | middleware, scope check |
| Provides | `PrincipalRevalidator` (`revalidate`) | 05 (passed in by 11 as a parameter of `ChangeFeed::read`) | end a held long-poll after revoke |
| Provides | `AccountGuard` (`ensure_active`) | 02 | refuse commits for a non-active account |
| Provides | `SignInService`, `SessionService`, `DeviceGrantService` | 11 | auth, session and device routes |
| Provides | `InviteService` | 11, 12 | invite routes, CLI (system scope) |
| Provides | `OperatorIdentityOps` | 12 | setup token, admin invite, admin reset (re-bind) |
| Provides (inherent method of the service, not in `contracts`) | `register_hooks` | 12 (only caller) | late, set-once list of `AccountLifecycleHook`s |
| Provides | `AccountAdmin` (`list_accounts`, `lock_active_admins`, `set_role`, `set_state`, `begin_deletion`, `finalize_deletion`) | 07 | admin screens, role and state changes, account deletion |
| Provides | `Janitor` (`IdentityMaintenance`) | 12 scheduler | expired states, grants, codes, sessions |
| Requires | `AccountLifecycleHook` (`initialize` only) | 02, 05, 07a (list registered late by 12) | account creation |
| Helpers | `AbuseLimiter` (08_ratelimit), `AuditWriter` (04_audit), `Clock` (03_clock), `Db` (07_db) | | |

`Janitor` is `name()` plus `run_cycle(now) -> CycleStats`, defined in `contracts`; the scheduler in the `sunnycloud` binary (12) runs every registered one at its configured interval (decision 16).

## 3. External API Specification (resources owned; exposed through 11_http_api)
| Endpoint | Method | Scope | Notes |
|---|---|---|---|
| `/api/v1/auth/providers` | GET | Public | enabled providers, `setup_required` |
| `/api/v1/auth/login-attempts` | POST | Public | begin sign-in; body carries invite/setup token (never in a URL); sets browser-binding cookie; 201 `{authorization_url}` |
| `/api/v1/auth/providers/{provider}/callback` | GET | Public | provider redirect target; 302 to return path or SPA error route |
| `/api/v1/auth/mobile-code-exchanges` | POST | Public | one-time code + PKCE verifier -> tokens |
| `/api/v1/auth/refresh-tokens` | POST | Public (refresh credential) | rotation |
| `/api/v1/sessions` / `/{id}` / `/current` | GET / DELETE | SessionOnly | list, revoke, logout |
| `/api/v1/identity-link-attempts` | POST | SessionOnly | link provider (never by email) |
| `/api/v1/device-grants` | POST | Public | start device flow (rate-limited per IP) |
| `/api/v1/device-tokens` | POST | Public | poll with `device_code` (RFC 8628 error codes) |
| `/api/v1/device-grants?user_code=` / `/{id}` | GET / PATCH | SessionOnly | view requester vs approver; `{decision}` |
| `/api/v1/devices` / `/{id}` | GET / DELETE | SessionOnly | list, revoke device |
| `/api/v1/devices/me` | DELETE | DeviceOnly (owner decision 2026-11-12) | a desktop revokes its own token (sign-out); a session principal is refused |
| `/api/v1/admin/invites` / `/{id}` | POST, GET / DELETE | Admin | invite management (member invites) |
Mobile sign-in result is a normal `sessions` row (`client_kind` android/ios) with refresh rotation, not a device token (the device grant is desktop only and yields a sync-only token).

## 4. Data Access Patterns
**Owned tables**: `accounts`, `identities`, `sessions`, `device_tokens`, `device_grants`, `invites` (incl. setup; with `grants_role`, `target_account_id`), `oauth_states` (with `code_challenge`), `login_codes`. Hot path: `authenticate` = one probe on a unique `*_hash` index per request, wrapped by an in-process cache of at most `session_cache_ttl` (2 s, keeps revocation inside 5 s). Evaluation of revoked/expired is in code, not in `WHERE`.
**`login_codes`** (decision 11): `code_hash` PK, `account_id`, `code_challenge`, `client_kind`, `expires_at`; created at the callback, consumed by `DELETE … RETURNING` at the exchange, which mints the session.
**Janitor** `IdentityMaintenance::run_cycle(now)`: expired `oauth_states`, `device_grants` 1 h after expiry/consumption, expired `login_codes`, sessions 30 d after revoke/expiry. Registered with the scheduler in the `sunnycloud` binary.
**Rollback**: identity migrations are part of `create_identity_tables`; account rows are never hard-deleted except by `AccountAdmin::finalize_deletion`. Changes made by the CLI process (disable, delete, reset) reach the API process's session cache and held polls through the 2 s cache TTL and the 10 s feed recheck; there is no cross-process push.

## 5. Implementation Details
**State**: stateless apart from the session cache and the in-memory abuse counters (rate-limit helper: sign-in steps 10/min/IP; device approvals 5/session, 20/account/h, 50/IP/h; invalid tokens). A restart resets counters; trips are audited once.
**Libraries**: openidconnect 4.1.0, oauth2 5.0.0, jsonwebtoken (id-token signature, Apple client secret ES256 in v1.1), reqwest (provider client with redirects off), SHA-256 via helper 05.
**Error handling**: typed enums above; provider timeouts surface as `ProviderUnavailable`, never partial accounts (single transaction). No catch-and-continue; audit failures are logged and the request still fails closed only for sign-in events marked mandatory.
**Email rule**: an email is read only when the allowlist is configured, only if `email_verified`, only at invite redemption; GitHub uses the per-address `verified` flag. Unverified (Apple relay, Facebook-like) is treated as no email (AC-55). With the allowlist set, a verified email that is not on it refuses the invite redemption (`InvitationRequired`, nothing created; owner decision 2026-11-13). A new account's display name is the provider's name claim, else its username (GitHub `login`, OIDC `preferred_username`), else `User` (owner decision 2026-11-13).
**Setup token**: valid only while `accounts` is empty; printed once to stdout by `ensure_setup_token` at API start (explicit exception to "no secrets in logs", documented in the install guide).

## 6. Configuration keys (all durations from config, tests use short values)
`public_url`, `providers[]` (key, kind, issuer, client_id, secret file), `email_allowlist[]`, `access_token_ttl` (15 min), `refresh_token_ttl` (30 d), `refresh_reuse_grace` (30 s), `session_cache_ttl` (2 s), `session_last_seen_interval` (1 min), `device_code_ttl` (10 min max), `device_poll_interval` (5 s), `oauth_state_ttl` (10 min), `login_code_ttl` (2 min), `invite_ttl` (7 d), `setup_token_ttl` (7 d), `provider_timeout` (10 s), abuse-limit numbers from AC-20, `geoip_db_path`, `allow_plain_http_dev` (off). Validation at start: at least one provider; absolute `public_url` with https unless dev flag.

## 7. Security notes
Tokens: 256-bit random with type prefix, only SHA-256 stored. `return_to` validated at store time and again at redirect (CHECK in table). `redirect_uri` built from `public_url`, never from Host. Callback 12 negative cases map one-to-one to `UntrustedState`/`TrustedFailure`. Device tokens do not rotate in v1.0 (revocable). Missing/expired/revoked credentials return identical responses. IDs and codes only in audit `detail`. Disabled or `deleting` accounts fail `authenticate`.

## 8. Dependency graph
Implement after: helpers, `contracts`. Parallel with: 05, 04. Blocks: 11, 12 and the integration tests of anything that needs a principal. No dependency on any other component crate.

## 9. ACs, tests, acceptance
**ACs [release]**: AC-19 [v1.0; Apple v1.1], AC-20 (sign-in and device-approval limits) [v1.0], AC-21 [v1.0], AC-23 [v1.0], AC-54 (server side: https public URL) [v1.0], AC-55 [v1.0], AC-56 [v1.0], AC-57 [v1.0], AC-58 (refresh rotation part; CSRF part is 11) [v1.0], AC-59 (authn half) [v1.0].
**Interaction ACs folded in**: AC-75 (single-use invite under concurrency) [v1.0]; AC-76 (device code exchanges once, never after expiry) [v1.0]; AC-79 (worker auth is 08's `WorkerAuthenticator`; user and device tokens never open internal routes, internal routes unreachable publicly, enforced with 11) [v1.0]. Test IDs for AC-75/76/79 are added in test-spec Step 5 (pending).
**Tests**: FT-P-62, FT-P-63 (v1.1), FT-P-64, FT-P-65, FT-P-66, FT-N-20..FT-N-25, FT-N-28..FT-N-30, FT-N-34 (v1.1), NFT-SEC-01..05, 07, 08, 10, 30, 38, NFT-RES-24, 26, 32.
**Acceptance (observable)**: unknown identity without invite creates no account; 12 bad callbacks yield non-2xx and no session; four concurrent refreshes give one successor; revoked session ends a held long-poll within 5 s; an unverified-email mock creates and links nothing; second device-code exchange fails; two parallel invite redemptions leave one account.

## 10. Logging
ERROR provider/DB failure; WARN refresh reuse, abuse-limit trip (IDs only); INFO sign-in, device approved, session revoked (also to audit log). Structured JSON, stdout. Never tokens, codes, emails.

## 11. Risks
Session cache of 2 s plus the long-poll recheck bound revocation latency; raising `session_cache_ttl` above 3 s breaks AC-21. Hook ordering at account creation must stay inside one short transaction. Apple flow is v1.1 and must not change the v1.0 `ProviderClient` trait.
