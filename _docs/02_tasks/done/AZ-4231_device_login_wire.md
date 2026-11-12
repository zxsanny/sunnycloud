# Device login wire and unavailable credentials in sync-core

**Task**: AZ-4231_device_login_wire
**Name**: Device login wire and credential availability
**Description**: Make sync-core's device login follow the server's documented answers, add `verification_uri` and `expires_in` to the grant answer, and let a host report that its credential is unavailable without the request being sent or the session being stopped
**Complexity**: 3 points
**Dependencies**: AZ-3828_identity_admin_handlers, AZ-3966_token_lifecycle, AZ-3959_http_sync_client_connect
**Component**: 13 sync_core (with 11 http_api and 14 desktop_daemon)
**Tracker**: AZ-4231
**Epic**: AZ-3650 (E-23)
**Ref**: found in AZ-3908 (batch 47, 2026-11-12); owner decisions 2026-11-12

## Problem

1. Device login cannot work end to end. sync-core decodes `POST /api/v1/device-tokens` as a 200 `{"status": ...}` body and requires `verification_uri` in the `POST /api/v1/device-grants` answer. The server (`contracts/11_http_api/api_v1_surface.md`) answers the poll with 200 `{access_token, token_type}`, or with 400 and the code `authorization_pending`, `slow_down`, `access_denied` or `expired_token`. It answers the grant with `{device_code, user_code, interval, expires_in}` and no `verification_uri`. `contracts/13_sync_core/sync_core_proto.md` lists `verification_uri` but not `expires_in`.
2. A host whose credential store cannot be read (a locked keyring, a call that hit its bound) has no way to say so. `CredentialProvider::bearer()` cannot fail, so the daemon sends an empty bearer. The server answers 401, `renew` calls `stop_auth()`, and the user is signed out until a new device login, although the stored token is valid.

## Owner decisions (2026-11-12)

- Follow the server contract: sync-core changes to the server's documented answers. The server adds `verification_uri` to the grant answer (RFC 8628 requires it, and the daemon needs it to open the browser) and keeps `expires_in`.
- An unreadable credential store pauses sync and never signs the user out. Only a 401 answered to a request that carried a token signs out.

## Outcome

- One grant and poll wire, stated identically in the surface contract, the OpenAPI document and `sync_core_proto.md`, and decoded by sync-core.
- `CredentialProvider::bearer()` can report `CredentialUnavailable`. sync-core then sends nothing, answers a typed `SyncError::CredentialUnavailable`, and does not renew or stop authentication. The daemon maps its store failures to it.

## Scope

### Included
- 11: the grant answer adds `verification_uri`. Its value is the web app's device approval page under `public_url` (16 description section 5, "Device approval page"). The page route is open point 1. The OpenAPI schema and the surface contract change with it.
- 13: decode the grant answer, including `expires_in`, and the four 400 poll codes as their typed outcomes. Make `bearer()` fallible (`host_traits.md`), and handle `CredentialUnavailable` in `transmit`, `is_device_credential` and `renew`.
- 14: `StoreCredentials::bearer` returns `CredentialUnavailable` when the store has no token or cannot be read. The login fills `expires_at` from `expires_in`.

### Excluded
- The web approval page itself (E-26).

## Acceptance Criteria

**AC-1: Grant answer complete**
Given a device grant start
When the server answers
Then the body holds `device_code`, `user_code`, `verification_uri`, `interval` and `expires_in`, and the surface contract, the OpenAPI document and `sync_core_proto.md` list the same fields

**AC-2: sync-core decodes the server's poll answers**
Given the four 400 codes and a 200 token answer
When sync-core polls
Then each 400 code becomes its typed outcome (pending, slow down, denied, expired) and the 200 answer yields the token; any other answer is a typed error

**AC-3: Login expiry known**
Given a grant with `expires_in`
When the daemon starts a login
Then `LoginStarted` carries `expires_at` (AZ-3908 AC-3)

**AC-4: End to end**
Given the real API routes with stub identity services
When sync-core runs a device login against them
Then it gets a token, with no shape mismatch

**AC-5: Unavailable credential never signs out**
Given a host whose credential store cannot be read
When sync-core makes a call
Then no request is sent, the call answers `CredentialUnavailable`, authentication is not stopped, and the next call after the store recovers carries the token; a 401 to a request that carried a token still signs out

## Constraints

- One protocol path: sync-core follows the server contract, and no second decoder is added.
- Every host implementing `CredentialProvider` is updated in the same change: daemon, mobile backup, WASM.

## Open points

1. Resolved 2026-11-12 (owner): the approval page is the SPA route `/device`, so `verification_uri` = `<public_url>/device`.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Approval page route not named (open point 1) | E-26 / owner | resolved 2026-11-12: `/device` | Medium |
