# Contract: authenticator

**Component**: 01_identity
**Producer task**: B04-03_authenticator_scopes_cache (E-15/c03)
**Consumer tasks**: E-21/c07 (auth extraction per route class), E-12/c05 (long-poll auth recheck), E-15/c06, E-15/c07, E-15/c10 (revocation paths), E-15/c12, E-15/c13
**Version**: 1.1.0
**Status**: draft (freezes when the `DeviceOnly` question in Non-Goals is answered)
**Last Updated**: 2026-11-04

## Purpose

How a caller's credential becomes a `Principal`, how the scope check works, and how fast a revocation becomes visible. The trait signatures live in `contracts` (E-00/c03); this file states the behaviour every caller may rely on.

## Shape

### Types

| Field / value | Type | Required | Description | Constraints |
|-------|------|----------|-------------|-------------|
| `Credential::Session(token)` | opaque string | one of two | session access token (cookie or bearer) | exactly two variants exist |
| `Credential::Device(token)` | opaque string | one of two | sync-only device token | no rotation in v1.0 |
| `Principal.account_id` | `AccountId` (UUID v7) | yes | owner of the credential | |
| `Principal.kind` | `Session` \| `Device` | yes | credential kind | |
| `Principal.session_or_device_id` | UUID | yes | `sessions.id` or `device_tokens.id` | |
| `Principal.role` | `user` \| `admin` | yes | account role at authentication time | |
| `Principal.label` | string | yes | device name or user-agent summary | display only |
| `Scope` | `Public` \| `SessionOnly` \| `SessionOrDevice` \| `Admin` | | route requirement | no worker scope |

### Methods

| Name | Signature (behavioural) | Errors | Blocking? |
|------|-----------|-----------------|-----------|
| `authenticate` | `Credential -> Principal` | `Unauthenticated` (one value for missing, unknown, malformed, expired, revoked, disabled account, deleting account) | async, one DB read on cache miss |
| `revalidate` | `&Principal -> bool` | none | async, answered from a cache entry no older than `session_cache_ttl` |
| `require` | `(&Principal, Scope) -> ()` | `Forbidden` | sync, no I/O |

### Scope table

| Scope | Member session | Admin session | Device token |
|-------|----------------|---------------|--------------|
| `Public` | ok | ok | ok |
| `SessionOnly` | ok | ok | Forbidden |
| `SessionOrDevice` | ok | ok | ok |
| `Admin` | Forbidden | ok | Forbidden |

`SessionOnly` covers at least approve-device, create-share, delete-account, link-provider and device/session management (AC-57). `SessionOrDevice` covers tree, transfer, feed and quota reads.

## Scope table per identity operation (AZ-3693, 2026-11-12)

The auth class each operation must declare in 11's route registry. The CI check `tests/it/registry/scopes.rs` (11_http_api) holds the same table, reads the generated OpenAPI document (`x-owner-method`, `x-auth-class`) and fails, naming the operation and its expected class, on any difference. 01 scope maps to 11 class one to one: `Public`, `SessionOnly`, `SessionOrDevice`, `Admin`.

| Operation (owner method) | Scope |
|---|---|
| `SignInService::list_providers`, `begin`, `complete`, `exchange_mobile_code` | `Public` |
| `SessionService::refresh` | `Public` |
| `DeviceGrantService::request`, `poll` | `Public` |
| `SessionService::list`, `revoke` | `SessionOnly` |
| `SignInService::begin_link` | `SessionOnly` |
| `DeviceGrantService::lookup_for_approval`, `decide`, `list_devices`, `revoke_device` | `SessionOnly` |
| `InviteService::create_invite`, `list`, `revoke` | `Admin` |
| AC-57, owned elsewhere: `ShareService::create` (06), `AccountDeletionService::request` (07) | `SessionOnly` |
| `DeviceGrantService::revoke_self` (`DELETE /api/v1/devices/me`) | 11 class `DeviceOnly` (owner decision 2026-11-12; 01 has no device-only scope value, so the check names 11's class directly) |

## Invariants

- Every dead credential yields the same `Unauthenticated` value and the same database work (one read on a unique hash index, no write); the reason is never exposed to the caller.
- After a session, device token or account is revoked or disabled, `revalidate` returns false and `authenticate` fails no later than `session_cache_ttl` + 50 ms (2 s default). A held long-poll that rechecks with `revalidate` therefore ends within 5 s (AC-21).
- `session_cache_ttl` is never above 3 s (start validation).
- No credential other than a session access token or a device token ever produces a `Principal`: worker bearers, refresh tokens, login codes and device codes all give `Unauthenticated` (AC-79).
- `require` reads only the principal; it never touches the database.

## Non-Goals

- Not covered: reading cookies or headers, mapping to HTTP 401/403 (E-21).
- Not covered: worker authentication (08 `WorkerAuthenticator`, internal listener only).
- Not covered: tree-level authorization (callers scope by `account_id`).
- Open: 11 declares a `DeviceOnly` class for `DELETE /api/v1/devices/me`. Until the owner adds a matching scope, the refusal of a session principal on that route is done by `DeviceGrantService::revoke_self` (E-15/c07), not by `require`.

## Versioning Rules

- **Breaking changes** (a scope removed or its row in the table changed, a new `Credential` variant, a field removed) require a new major version and a deprecation path for consumers.
- **Non-breaking additions** (a new scope consumers do not use yet, a new optional principal field) require a minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| valid-session | live session access token | `Principal{kind: Session}` | IT-01-1 positive |
| dead-credentials | unknown, expired, revoked, disabled, deleting, malformed, empty | same `Unauthenticated`, 1 indexed read, 0 writes | IT-01-1 |
| revocation-latency | revoke with TTL 200 ms | `revalidate` false within 250 ms | IT-01-2 |
| scope-matrix | 3 principals x 4 scopes | as the scope table | IT-01-3 |
| foreign-bearer | worker bearer, refresh token, login code, device code | `Unauthenticated` | ST-01-4 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B04) |
| 1.1.0 | 2026-11-12 | Scope table per identity operation and its CI check | AZ-3693 |
