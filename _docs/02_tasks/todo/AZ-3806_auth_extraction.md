# Auth extraction per `AuthClass` (`Principal`, 401/403, worker route auth)

**Task**: AZ-3806_auth_extraction
**Name**: Auth extraction
**Description**: Read the session cookie or bearer token, turn it into a `Principal` through 01's `Authenticator` (or a worker identity through 08's `WorkerAuthenticator` on worker routes), and accept or refuse per the route's auth class before any body byte is read
**Complexity**: 3 points
**Dependencies**: AZ-3810_error_mapper, AZ-3827_route_registry_openapi, AZ-3768_contracts_part1, AZ-3770_contracts_part3, AZ-3766_sc_ids
**Component**: 11 http_api
**Tracker**: AZ-3806
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/01_identity/authenticator.md` — `authenticate`, `require`, the scope table and the one `Unauthenticated` answer (E-15/c03).
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — `WorkerAuthenticator` with its single `Forbidden` (E-00/c05).
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — auth classes and credential transport (E-21/c11).
- `_docs/02_document/contracts/11_http_api/problem_details.md` — 401 and 403 bodies (E-21/c09).
- `_docs/02_document/contracts/shared_ids_hashing/sc_ids.md` — token kinds (E-04/c01).

## Problem

Every protected route must know who is calling and whether that kind of credential may use it. Device tokens must not reach session-only operations (AC-57); a revoked credential must stop working within 5 s (AC-21); an attacker must not tell a revoked token from an unknown one; worker routes must refuse every user credential the same way (AC-79). Large HTTP/2 windows mean an unauthenticated peer must not get its body read (AC-59).

## Outcome

- Each route's auth class is enforced in layer 8: `Public`, `Session`, `SessionOrDevice`, `DeviceOnly`, `SessionOnly`, `Admin`, `ShareGrant` (resolved by 06 in the handler), `Worker`.
- No credential on a protected route gives 401; revoked, expired or unknown gives 401 with one body; wrong kind gives 403; worker routes give one uniform 403 for any non-worker credential.
- The `Principal` reaches the handler with the right account id, kind and session or device id; the body is untouched until auth passes.

## Scope

### Included
- Reading the `__Host-` session cookie and `Authorization: Bearer`; choosing `Credential::Session` or `Credential::Device` by the token's kind.
- One `Authenticator::authenticate` call per request, then `require` with the route's scope; `DeviceOnly` and `Session` as 11 classes over the principal's kind.
- Worker routes: `WorkerAuthenticator::authenticate(bearer)` only; user credentials are never passed to it and worker bearers never to 01.
- Dropping the unread body (flow-control credit released) when auth fails.
- Passing the principal (and for admins `retained_ok`) to handlers.

### Excluded
- Session logic, cache and revocation timing (E-15/c03); share token and grant checks (06, E-21/c14); CSRF (E-21/c08).

## Acceptance Criteria

**AC-1: Missing and dead credentials (AC-21)**
Given routes of every protected class
When they are called with no credential, a revoked session, a revoked device token, an expired session and an unknown token
Then each answers 401, and the bodies for revoked, expired and unknown are identical apart from `request_id`

**AC-2: Wrong kind is 403 (AC-57)**
Given a device token, a non-admin session and an admin session
When the device token calls approve-device, create-share, delete-account, link-provider and device revoke by id; a session calls `DELETE /api/v1/devices/me`; the non-admin calls an admin route
Then each answers 403, and the admin session passes the admin route

**AC-3: Principal reaches the handler**
Given a session, a device token and an admin session on a `SessionOrDevice` route whose stub records its input
When each calls the route
Then the stub receives `Principal{account_id, kind, session_or_device_id}` with the matching account and kind, and `retained_ok` is true only for the admin

**AC-4: Public routes accept everyone**
Given a `Public` route
When it is called with no credential, a session, a device token and a revoked token
Then all four reach the handler

**AC-5: Worker routes refuse user credentials uniformly (AC-79)**
Given a worker route
When it is called with no `Authorization`, a session bearer, a session cookie, a device token and a wrong worker secret
Then all five answer 403 with bodies equal apart from `request_id`, 01's `Authenticator` is never called, and the worker bearer reaches the handler

**AC-6: Auth before body (AC-59)**
Given an unauthenticated 2 MiB block PUT, sent 100 times
When the requests arrive
Then each gets 401 or 403 without its body being read, and server memory grows by less than 10 MB over the 100 requests

## Non-Functional Requirements

**Performance**
- One authenticator call per request; no database work in 11.

**Security**
- The reason for a 401 is never in the answer; tokens never reach logs.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Class by principal-kind table for 8 classes x 4 principal types | Accept or 401/403 as the contract states |
| AC-5 | Credential routing: worker routes never call 01, user routes never call 08 | Call counts 0 on the other port |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S, scripted `Authenticator` | Dead credentials (IT-11-10) | 401, identical bodies | AC-21, FT-N-30 |
| AC-2 | Mode S | Wrong kind (IT-11-10) | 403 on each listed operation | AC-57, NFT-SEC-08 |
| AC-3 | Mode S, recording stub | Principal passing (IT-11-10) | Right account and kind | AC-59 |
| AC-4 | Mode S | Public class (IT-11-10) | Everyone passes | |
| AC-5 | Mode S, internal listener | Worker auth (IT-11-9, auth part) | Uniform 403; worker passes | AC-79, NFT-SEC-50 |
| AC-6 | Mode S | Unauthenticated block PUTs (IT-11-19, last line) | Refused, body dropped, RSS < 10 MB growth | NFT-SEC-15 |

## Constraints

- 11 never inspects a token beyond choosing its kind; validity is 01's or 08's answer.

## Risks & Mitigation

**Risk 1: Kind picked wrongly**
- *Risk*: a device token passed as a session credential fails as unknown, or vice versa.
- *Mitigation*: AC-3 with both kinds; the token kind marker is shared with 01 through `sc_ids`.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `DeviceOnly`: 01's `Scope` has four values and leaves the session refusal on `DELETE /devices/me` to `DeviceGrantService::revoke_self`; 11 declares a `DeviceOnly` class. Where the refusal lives is open; AC-2 asserts the HTTP answer only | E-15, E-21 owners | open | Medium |
| How 11 tells a session bearer from a device bearer: tokens carry a type prefix (01, data_model §1), but the prefix strings are not specified (`sc_ids` open point 1) | E-04, E-15 owners | open | Medium |
| `Principal` field naming: 01 `session_or_device_id`, 11 `session_id`; this task uses 01's | E-21 owner | open | Low |
| Session cookie name after the `__Host-` prefix is not specified | E-15, E-21 owners | open | Low |

## Contract

This task implements the auth-class and credential-transport rows of the contract at `_docs/02_document/contracts/11_http_api/api_v1_surface.md` (producer E-21/c11).
Consumers MUST read that file — not this task spec — to discover the interface.
