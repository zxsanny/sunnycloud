# 06_sharing — Share links

## 1. High-Level Overview

**Purpose**: Create, list, revoke and resolve revocable share links to a file, for visitors without an account (v1.0 sharing is by link only).

**Single responsibility**: link lifecycle and visitor access decisions (token lookup, uniform not-found, password check, access grant, abuse limits, audit). It resolves *what* a link points at through `TreeReader::resolve_live_file` (implemented by 02_tree) and never serves bytes: byte streaming, Range and the content-serving headers belong to 03_transfer / 11_http_api, which compose the two.

**Architectural pattern**: service over one repository; stateless constant-work lookup; short-lived signed access grant instead of a grant table.

**Scope in**: `share_links`, token generation/hash, expiry, optional Argon2id password, revoke, uniform 404 behavior, per-link and per-IP failure counters, audit events, the account-erase hook for `share_links`.
**Scope out**: content streaming, allowlisted inline types, `nosniff`/CSP (11 + 03); node moves/renames (links follow the node id automatically); vault fragment keys (v1.2: the client appends the key to the URL fragment; the server stores nothing extra and has no special case).

**Upstream dependencies**: `contracts` ports only (see Ports); helpers (ids_hashing, clock, audit, config, error, ratelimit, db). The owner principal is checked at the boundary by 11 through 01's `Authenticator`; 06 receives a plain `AccountId`.
**Downstream consumers**: through ports only; see Ports.
**No cycle**: 02 does not know sharing; purged targets remove links by `ON DELETE CASCADE`.

## 2. Internal Interfaces

### ShareService (owner side)
| Method | Input | Output | Errors |
|---|---|---|---|
| `create` | account, node id, `expires_in?` (default `share_default_ttl`), `password?` | `CreatedShare{id, token (plaintext, only here), url_path, expires_at, has_password}` | `TargetNotFound` (`Gone`: other accounts' ids, tombstoned, purged), `TargetNotShareable` (`LiveFile.kind = Folder`), `ExpiryOutOfRange` |
| `list` | account, optional node id | `[ShareInfo]` (no token) | none |
| `revoke` | account, share id | `()` idempotent | `NotFound` |
Scope for all: `SessionOnly` (device tokens get 403, AC-57). Share links target files only in v1.0 (decision 15); the schema would allow folders, `create` refuses them with `TargetNotShareable` (a folder is told apart from a gone node by `kind`, not by the error of a lookup). Vault files cannot be link-shared in v1.2 either (`TargetNotShareable`; how 06 learns a file is a vault file is defined with 20).

### PublicShareService (visitor side, no account)
| Method | Input | Output | Errors / outcomes |
|---|---|---|---|
| `open` | token, client IP | `ShareOpen::Available{name, size, media_type, expires_at}` \| `PasswordRequired` \| `NotFound` | `RateLimited{retry_after}` |
| `verify_password` | token, password, IP | `AccessGrant{value, expires_at}` | `NotFound`, `WrongPassword`, `RateLimited` |
| `authorize_content` | token, `Option<AccessGrant>`, IP, `starts_download: bool` | `ShareContent{account_id, node_id, revision_id}` | `NotFound`, `PasswordRequired`, `RateLimited` |
`NotFound` is one variant for unknown, expired, revoked, tombstoned or purged: identical status, body and headers, and the lookup does the same work in each case (one indexed read by hash, then decisions in code, constant-time branch padding) so the median timing differs by under 1 ms (AC-12). `PasswordRequired` is returned only for a live password link. `authorize_content` re-checks the link and the target on every content request with no cache older than `share_lookup_cache` (1 s), so revocation or file deletion answers `NotFound` within 5 s (AC-12, AC-77); it returns the *current* revision so a save-in-place edit is visible and the link survives (AC-34). The caller (11/03) streams `revision_id` for `account_id`. `starts_download` is set by 11 when the request has no `Range` header or its range starts at byte 0; only then is `share_accessed` audited, so one download start writes one event however many Range requests follow. A link to a folder (cannot exist in v1.0) or a gone node answers `NotFound` here.

### AccountLifecycleHook (implemented here)
`initialize(tx, account)` does nothing. `erase_batch(account, limit) -> EraseProgress{remaining}` deletes the account's `share_links` rows (tens at most, normally one batch). Links stop resolving once their rows are gone, at most one `account_delete_interval` after the deletion request; a tombstone behaves as missing meanwhile. Revoking per node is not needed.

## Ports (crate `contracts`)
| Direction | Port | Counterpart | Used for |
|---|---|---|---|
| Provides | `ShareService`, `PublicShareService` | 11 | owner routes, public routes (11 then streams `revision_id` through 03's `DownloadService`) |
| Provides | `AccountLifecycleHook` impl | 01 (initialize), 07 (erase_batch) | delete the account's links at account deletion |
| Requires | `TreeReader` (`resolve_live_file`, `get_node`) | 02 | live file lookup on create and on every content request (`kind` separates folder from gone; `get_node` supplies the name) |
| Helpers | `AbuseLimiter` (08_ratelimit), `AuditWriter` (04_audit), `Clock`, `Db` | | |

## 3. External API Specification (routed by 11_http_api)
| Endpoint | Method | Auth | Rate limit | Description |
|---|---|---|---|---|
| `/api/v1/shares` | POST, GET | SessionOnly | normal | create (201, token shown once), list own |
| `/api/v1/shares/{id}` | DELETE | SessionOnly | normal | revoke |
| `/api/v1/public-shares/{token}` | GET | Public | 100 invalid tokens/min/IP | metadata or uniform 404 / `password_required` |
| `/api/v1/public-shares/{token}/password-verifications` | POST | Public | 10 wrong/link/10 min | body `{password}` -> grant (sent as an HttpOnly, path-scoped cookie by 11) |
| `/api/v1/public-shares/{token}/content` | GET | Public (+grant) | shared with above | bytes via 03; Range allowed; `share_accessed` audited once per download start (no `Range` header or a range starting at 0) |
Token in the path is required by shareable URLs, so 11 must redact `/public-shares/*` segments from access logs and Caddy logs (NFT-SEC-16). `password_required` response shape: HTTP 401 with `{code: "password_required"}` (closes test-spec gap G-9).

## 4. Data Access Patterns
**Owned table**: `share_links` (`id`, `account_id`, `node_id` composite FK `ON DELETE CASCADE`, `token_hash` UNIQUE, `password_hash`, `expires_at` NOT NULL, `revoked_at`, `created_at`).
| Query | Frequency | Hot | Index |
|---|---|---|---|
| Lookup by `token_hash` | High on visits (every content request) | Yes | UNIQUE `token_hash` |
| Resolve target through `TreeReader` (`resolve_live_file` + `get_node`, 2 PK reads) | per visit | Yes | PKs |
| List by account / node | Low | No | `(account_id)`, `(node_id)` |
Cache: at most 1 s on lookup result (config), invalidation by TTL only. Cross-account token resolution runs in the system RLS scope before the owner is known; this is the single place outside admin/janitors allowed to do so and it is confined to the lookup function. Storage: tens of thousands of rows at most. **Rollback**: none; revoke is a column.

## 5. Implementation Details
**Token**: 256-bit random with prefix `sc_shr_`, SHA-256 stored (helper 05). **Password**: Argon2id PHC string; verification limited by a semaphore (`share_password_max_concurrency`) so password attempts cannot exhaust CPU (hash-cost DoS, NFT-SEC-06); not performed for tokens that do not exist.
**Access grant**: stateless value `HMAC(share_grant_key, token_hash ‖ expires_at)` with `share_grant_ttl`; bound to that link only, revoked implicitly because `authorize_content` re-reads the link. `share_grant_key` is a Docker secret (decision 15); the server refuses to start without it.
**Abuse counters** (in memory, helper 08 `AbuseLimiter`): 10 wrong passwords per link per 10 min; 100 invalid tokens per minute per IP; trips are audited once.
**Libraries**: `argon2`, `hmac`/`sha2`, `subtle` (constant-time compare). **Errors**: typed variants above; no catch-and-continue; audit write failure is logged and does not leak into the response.

## 6. Configuration keys
`share_default_ttl` (7 d), `share_max_ttl` (365 d; a longer `expires_in` gives `ExpiryOutOfRange`, decision 15), `share_lookup_cache` (1 s), `share_grant_ttl` (10 min), `share_wrong_password_limit` (10) and `_window` (10 min), `share_invalid_token_limit` (100/min/IP), `share_password_max_concurrency`, Argon2id parameters, `share_grant_key` (secret). Tests use short TTLs and windows.

## 7. Security notes
Only hashes at rest, tokens never logged. Missing/expired/revoked indistinguishable from outside. The visitor path never reveals the owner, account id or path, only name, size and type of the target. Content decisions (inline allowlist image/jpeg, png, gif, webp, avif, video/mp4, webm; else attachment, `nosniff`, sandbox CSP) are applied identically to share content by 11 (AC-47). Audit events `share_created`, `share_revoked`, `share_accessed` with IDs only. Device tokens cannot create (403).

## 8. Dependency graph
Implement after: helpers, `contracts`. Depends on no component crate; integration tests run against the real 02 behind `TreeReader` (no fake store). Blocks: 11 public routes, 16_web_app. No dependency on 03 (11 composes them).

## 9. ACs, tests, acceptance
**ACs [release]**: AC-12 [v1.0]; AC-20 (share password and invalid-token limits) [v1.0]; AC-34 (link survives save-in-place) [v1.0]; AC-47 (served-type rules, shared with 11) [v1.0]; AC-57 (create-share is session-only) [v1.0].
**Interaction AC folded in**: AC-77 (link follows rename/move; delete answers missing within 5 s) [v1.0]. Test ID added in Step 5 (pending).
**Tests**: FT-P-36, FT-P-37, FT-P-38, FT-P-39, FT-P-57; FT-N-09, FT-N-10, FT-N-11, FT-N-24; NFT-SEC-06, 16, 17, 18, 19; NFT-RES-25 (clock skew on expiry).
**Acceptance (observable)**: link opens in at most 2 s and returns the original bytes; unknown, expired and revoked tokens return byte-identical responses with median timing within 1 ms; 11th wrong password in 10 min gets 429 with `Retry-After`; revoke or file delete makes the link 404 within 5 s; rename/move and save-in-place keep it working; token cannot be recovered from the database or logs.

## 10. Logging
ERROR DB failure; WARN abuse trip (link id or IP hash, never token); INFO create/revoke (IDs). Structured JSON.

## 11. Risks
Folder links are out of scope for v1.0 (decision 15). Path-borne tokens depend on log redaction in 11. Constant-work lookup depends on a measured test (median within 1 ms) on the REF-SRV. Account deletion removes links through the erase hook driven by 07; 07 runs this hook first, so a deleting account's links stop within one `account_delete_interval` (5 s).
