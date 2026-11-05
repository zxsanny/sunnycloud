# Contract: contracts part 1 — registries, lifecycle, 01 identity, 07 admin, 07a quota ports

**Component**: `contracts` (server-only port crate, E-00)
**Producer task**: E-00/c03 — `_docs/02_tasks/todo/B01-12_contracts_part1.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-00/c04 (part 2), E-00/c05 (part 3); other batches: E-10..E-22, E-30 (implementers and callers)
**Version**: 1.1.0
**Status**: draft (frozen when the producer task is accepted)
**Last Updated**: 2026-11-05

## Purpose

The port traits and shared DTOs that every server component codes against for the late-registered lists (janitors, erase hooks, blob-owner cleanup, doctor, health, readiness), for identity (01), admin (07) and the quota ledger (07a). Signatures only; no implementation. ADR-015: components depend on these traits, never on each other.

Sources: `canonical-names.md` § Ports; `components/01_identity/description.md` §2 and Ports; `components/07_admin/description.md` §2-3; `components/07a_quota/description.md` §2-3; `components/12_operator_cli/description.md` §2, §6; `components/11_http_api/description.md` §2, §10; `epics/E-09_cross-cutting-observability.md` (readiness body).

## Shape

### Conventions (all three parts)

- Every port is a trait that is object safe and `Send + Sync`, so the composition root (12) holds it as `Arc<dyn Port>`. All methods are async unless marked `sync`.
- Ids are `Uuid` values named by role (`account`, `node_id`, `blob_id` ...), as the helper specs type them. Time is `Timestamp` (`sc_clock`). Tokens are `RawToken` / `UserCode` (`sc_ids`); a DTO that carries one prints a placeholder in `Debug`. Wire codes are `ErrorCode` (`sc_error`).
- A method that joins the caller's transaction takes `tx: &mut DbTx` (`sc_db`; no public constructor). Every other method opens its own short transaction inside the provider.
- Each port has its own error enum whose variants are exactly the ones listed below; a method whose source lists no typed failure has only the tier-3 `Unexpected` variant (helper 02). Providers convert their enums to `sc_error::BusinessError`; the HTTP status is chosen only by 11.
- No HTTP, sqlx or framework types appear in any signature.

### Registries and lifecycle (registered late by 12)

| Port | Method | Signature | Errors |
|---|---|---|---|
| `Janitor` | `name` | `sync fn name(&self) -> &'static str` | none |
| | `run_cycle` | `(now: Timestamp) -> CycleStats` | `Unexpected` |
| `AccountLifecycleHook` | `initialize` | `(tx: &mut DbTx, account: Uuid) -> ()` | `Unexpected` |
| | `erase_batch` | `(account: Uuid, limit: u32) -> EraseProgress` | `Unexpected` |
| `BlobOwnerCleanup` | `remove_owner_rows` | `(tx: &mut DbTx, blob_id: Uuid) -> ()` | `Unexpected` |
| `DoctorCheck` | `name`, `tags` | `sync` -> `&'static str`, `&[DoctorTag]` | none |
| | `run` | `(ctx: &DoctorContext) -> Vec<Finding>` | none (problems are findings) |
| `HealthContributor` | re-export | declared in `sc_observability` (`shared_observability/sc_observability.md`); `name`, `check() -> HealthItem` | none |
| `ReadinessCheck` | re-export | declared in `sc_observability`; `name`, `check() -> ReadinessItem` | none |

| DTO | Fields / variants |
|---|---|
| `EraseProgress` | `remaining: u64` (0 means this hook has nothing left for the account) |
| `CycleStats` | open: fields not specified (see Open points) |
| `DoctorTag` | `Restore`, `PreFlight` |
| `DoctorContext` | the `doctor` options of 12 §3: `pre_flight: bool`, `backup_interval: Option<Duration>`, `attest_backup: bool` (derived, see Open points) |
| `Finding` | `level: Ok \| Warn \| Fail`, `message`, `remedy` (no secrets) |
| `HealthItem`, `ReadinessItem` | re-exported from `sc_observability` (owner decision 2026-11-05) |

### 01 identity

| Port | Method | Input -> Output | Errors |
|---|---|---|---|
| `Authenticator` | `authenticate` | `Credential` -> `Principal` | `Unauthenticated` (one variant for missing, unknown, expired, revoked) |
| | `require` (`sync`) | `&Principal, Scope` -> `()` | `Forbidden` |
| `PrincipalRevalidator` | `revalidate` | `&Principal` -> `bool` | none |
| `AccountGuard` | `ensure_active` | `tx: &mut DbTx, account` -> `()` | `AccountNotActive` |
| `SignInService` | `begin` | `BeginSignIn{provider, return_to, client_kind, invite_token?, setup_token?, code_challenge?, host}` -> `AuthorizationRedirect{url, browser_binding}` | `UnknownProvider`, `InvalidReturnPath`, `HostMismatch`, `RateLimited{retry_after}` |
| | `complete` | `provider, callback_params, browser_binding, host` -> `SignInOutcome` | `UntrustedState`, `TrustedFailure{code}`, `InvitationRequired`, `ProviderUnavailable`, `RateLimited{retry_after}` |
| | `begin_link` | `&Principal, provider` -> `AuthorizationRedirect` | as `begin` |
| | `complete_link` | `&Principal, provider, callback_params, browser_binding, host` -> `Linked` | as `complete`, plus `IdentityTaken` |
| | `exchange_mobile_code` | `code, code_verifier` -> `SessionTokens` | `InvalidCode` |
| | `list_providers` | `()` -> `ProviderList{providers: Vec<ProviderInfo>, setup_required}` | none |
| `SessionService` | `refresh` | `refresh_token` -> `SessionTokens` | `ReuseDetected`, `Unauthenticated` |
| | `revoke` | `account, session_id, reason` -> `()` | `Unexpected` |
| | `revoke_all` | `account, reason` -> `()` | `Unexpected` |
| | `list` | `account` -> `Vec<SessionInfo>` | `Unexpected` |
| | `touch` | `&Principal` -> `()` (at most once a minute) | `Unexpected` |
| `DeviceGrantService` | `request` | `device_name, platform, requester_ip` -> `DeviceAuthorization{device_code, user_code, interval, expires_in}` | none |
| | `poll` | `device_code` -> `DeviceToken` (once) | `AuthorizationPending`, `SlowDown`, `AccessDenied`, `ExpiredToken` |
| | `lookup_for_approval` | `&Principal, user_code, approver_ip` -> `ApprovalView{grant_id, expires_at, device, requester_ip, requester_location, approver_ip, approver_location, mismatch}` (`grant_id` is the id `decide` takes; `expires_at` is the code's expiry, owner decision 2026-11-05) | `UnknownCode`, `RateLimited{retry_after}` |
| | `decide` | `&Principal, grant_id, Decision{Approve \| Deny}` -> `()` | `UnknownCode`, `Expired`, `AlreadyDecided` |
| | `list_devices` | `account` -> `Vec<DeviceInfo>` | none |
| | `revoke_device` | `account, device_id` -> `()` | `NotFound` |
| | `revoke_self` | `&Principal` (device) -> `()` | none |
| `InviteService` | `create_invite` | `admin_account, note` -> `InviteToken` (plaintext, once) | `Unexpected` |
| | `list` | `admin_account` -> `Vec<InviteInfo>` | `Unexpected` |
| | `revoke` | `admin_account, invite_id` -> `()` | `Unexpected` |
| `OperatorIdentityOps` (system scope) | `ensure_setup_token` | `()` -> `Option<RawToken>` (only while zero accounts) | `Unexpected` |
| | `create_admin_invite` | `()` -> `InviteToken` | `Unexpected` |
| | `reset_admin` | `account` -> `InviteToken` (operator invite with `target_account_id`; the admin's sessions are revoked) | `Unexpected` |
| `AccountAdmin` | `list_accounts` | `filter, page` -> `Vec<AccountInfo>` | `Unexpected` |
| | `lock_active_admins` | `tx: &mut DbTx` -> `Vec<Uuid>` | `Unexpected` |
| | `set_role` | `tx: &mut DbTx, account, Role` -> `()` | `Unexpected` |
| | `set_state` | `tx: &mut DbTx, account, AccountState{Active \| Disabled}` -> `()` (a disabled account's sessions and device tokens are revoked in the same transaction) | `Unexpected` |
| | `begin_deletion` | `tx: &mut DbTx, account` -> `()` | `Unexpected` |
| | `finalize_deletion` | `tx: &mut DbTx, account` -> `()` | `Unexpected` |

| DTO | Fields / variants |
|---|---|
| `Credential` | `Session(token)`, `Device(token)` |
| `Principal` | `account_id, kind: Session \| Device, session_or_device_id, role: Role, label` |
| `Scope` | `Public`, `SessionOnly`, `SessionOrDevice`, `Admin` |
| `Role` | `User`, `Admin` (data_model §4.1; see Open points for `member`) |
| `SignInOutcome` | `Session{tokens: SessionTokens, return_to}`, `MobileCode{code, return_to}` |
| `SessionTokens` | `access, refresh, expires_in` (tokens print as placeholders) |
| `DeviceToken` | the sync-only device token (printed as placeholder) |
| `ClientKind` | the `client_kind` of `begin` (`web`, `android`, `ios`, `desktop`); `code_challenge` required for mobile kinds |
| `ProviderInfo`, `SessionInfo`, `DeviceInfo`, `InviteInfo`, `AccountInfo` | ids and display fields only, no token (field lists open, see Open points) |

### 07 admin

| Port | Method | Input -> Output | Errors |
|---|---|---|---|
| `AdminService` | `list_users` | `Actor, page` -> `Vec<AdminUserView{account, role, state, last_login, used, limit}>` | `Unexpected` |
| | `update_user` | `Actor, account, role?, state?` -> `()` | `LastAdmin`, `NotFound` |
| | `set_quota` | `Actor, account, limit: Option<i64>` -> `()` | `NotFound`, `InvalidLimit` |
| | `health` | `Actor` -> `HealthReport{items: Vec<(name, HealthItem)>}` | none |
| `AccountService` | `view` | `&Principal` -> `AccountView{id, display_name, role, identities, quota, root_node_id}` | `Unexpected` |
| | `quota_view` | `&Principal` -> `QuotaView` | `Unexpected` |
| `AccountDeletionService` | `request` | `&Principal, confirmation` -> `DeletionAccepted{deadline: Timestamp}` | `Forbidden` (device token), `LastAdmin`, `ConfirmationMismatch` |
| `DevSeed` | `seed` | `()` -> `()` | a refusal when `environment = production` (variant name open) |

`Actor` = `Admin(Principal)` or `Operator` (CLI). Every mutating call writes an `admin_action` audit row inside the provider.

### 07a quota

| Port | Method | Input -> Output | Errors |
|---|---|---|---|
| `QuotaLedger` | `reserve` | `tx: &mut DbTx, account, bytes: i64` -> `Reserved` | `QuotaExceeded{limit, used, reserved}` |
| | `release_reservation` | `tx: &mut DbTx, account, bytes: i64` -> `()` | none (clamped at zero) |
| | `adjust_used` | `tx: &mut DbTx, account, delta: i64` -> `()` | `InvariantViolation` (result would be negative) |
| `QuotaAdmin` | `view` | `account` -> `QuotaView{limit_bytes: Option<i64>, effective_limit, used, reserved}` | `NotFound` |
| | `views` | `&[Uuid]` -> map account -> `QuotaView` (unknown ids omitted) | none |
| | `set_limit` | `account, Option<i64>` (None = installation default) -> `()` | `NotFound`, `InvalidLimit` (valid 0 <= limit <= 2^53) |
| | `recompute` | `account, live_bytes: i64, reserved_bytes: i64, repair: bool` -> `Drift{stored, computed}` | none |

## Invariants

- The crate holds signatures and plain data only: no function bodies beyond trivial accessors, no `todo!()`, no I/O.
- `contracts` imports only layer-1 helper types (`DbTx` alone from `sc_db`; `HealthContributor`, `ReadinessCheck` and their items from `sc_observability`, re-exported unchanged); it never imports a component crate, `media-shared`, `sync-core*`, sqlx or an HTTP crate, and no client crate imports it (crate-graph test).
- Every port in this file is usable as `Arc<dyn Port>` shared across threads.
- `AccountGuard::ensure_active`, the `AccountAdmin` mutators, the `QuotaLedger` methods, `AccountLifecycleHook::initialize` and `BlobOwnerCleanup::remove_owner_rows` take the caller's `&mut DbTx`; nothing else does.
- Only 01 calls `AccountLifecycleHook::initialize`; only 07 calls `erase_batch` (canonical-names § Ports). The contract carries no list order; 12 registers it.
- No DTO exposes a raw token outside the one call that mints it, and its `Debug` output is a placeholder (NFT-SEC-16).

## Non-Goals

- Not covered: any behaviour (token rotation, last-admin rule, reserve SQL); the HTTP mapping of the error variants (11); per-route scopes (11 route registry).
- Not covered: 02, 03, 05 ports (part 2) and 04, 06, 08, 20 ports (part 3).

## Versioning Rules

- Breaking changes (method or field renamed or removed, type changed, required/optional flipped) need a new major version, a `contracts` change task in the provider's epic (module-layout Rule 6) and notice to every implementer.
- Adding an optional DTO field or an error variant callers already treat as unexpected is a minor bump.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| listing-part1 | public traits of the crate after part 1 | every port of this file present, 0 missing | listing test, extended by parts 2 and 3 |
| fake-per-port | a recording double for each trait in a test | compiles and can be held as `Arc<dyn Port>` | proves components can code against fakes |
| dbtx-sealed | code outside `sc_db` constructs `DbTx` | does not compile (`compile_fail` doc test) | |
| token-debug | `Debug` of `SessionTokens`, `InviteToken`, `DeviceToken`, `DeviceAuthorization` with known token values | output contains none of the token bytes | NFT-SEC-16 |
| no-http-types | crate dependency list | no axum, hyper, http, sqlx | crate-graph test |

## Open points (flagged in the producer task)

1. `CycleStats` fields, `ProviderInfo`/`SessionInfo`/`DeviceInfo`/`InviteInfo`/`AccountInfo` field lists and the `list_accounts` filter are not specified by 01, 07 or 12.
2. `ReadinessCheck` has no method list in any spec; the shape above is inferred from the `/readyz` body of E-09 (`{name, status, detail}`).
3. `DoctorContext` fields are derived from the `doctor` options in 12 §3; 12 names only `run(ctx)`.
4. `Role`: data_model §4.1 says `user`/`admin`; 01 invites carry `grants_role` `member`/`admin`.
5. `AccountDeletionService::request` takes a principal, but 12 also calls it for `admin user delete` (operator, no principal). The `DevSeed` refusal has no variant name in 07.
6. 01 §2 lists `revalidate` under `Authenticator`; the Ports table and canonical-names split it into `PrincipalRevalidator`. This contract follows the Ports table.
7. Where 01 lists no error for a method (`reset_admin`, the `AccountAdmin` mutators, invite `revoke`), only `Unexpected` is declared; a provider that needs a typed failure (for example an unknown account) adds it through a minor version.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
| 1.1.0 | 2026-11-05 | `HealthContributor`, `ReadinessCheck`, `HealthItem`, `ReadinessItem` moved to `sc_observability` and re-exported here; `ApprovalView` gains `grant_id` and `expires_at` (owner decisions 2026-11-05) | decompose Step 4 |
