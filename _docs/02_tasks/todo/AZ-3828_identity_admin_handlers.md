# Handlers for auth, sessions, devices, accounts and admin (E-15, E-17 ports)

**Task**: AZ-3828_identity_admin_handlers
**Name**: Identity, account and admin handlers
**Description**: Register and implement the thin handlers for sign-in, mobile code exchange, refresh, sessions, identity linking, device grants and devices, the own account and quota, admin users, quotas, health and invites: bind, validate, one port call, map
**Complexity**: 5 points
**Dependencies**: AZ-3803_body_limits_timeouts, AZ-3806_auth_extraction, AZ-3808_csrf_origin_check, AZ-3810_error_mapper, AZ-3827_route_registry_openapi, AZ-3768_contracts_part1
**Component**: 11 http_api
**Tracker**: AZ-3828
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c12

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `SignInService`, `SessionService`, `DeviceGrantService`, `InviteService`, `AdminService`, `AccountService`, `AccountDeletionService` (E-00/c03).
- `_docs/02_document/contracts/01_identity/authenticator.md` — principal and scope rules (E-15/c03).
- `_docs/02_document/contracts/01_identity/operator_identity_ops.md` — invite behaviour behind the admin invite routes (E-15/c08).
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — the operation rows of these groups (E-21/c11).
- `_docs/02_document/contracts/11_http_api/problem_details.md` — error mapping (E-21/c09).

## Problem

01 and 07 own sign-in, sessions, devices, the account and admin operations, but expose them only as ports. Clients reach them over HTTP: the browser through cookies and redirects, the mobile app through a one-time code and bearer tokens, the desktop daemon through the device grant. The HTTP side must carry cookies, redirects, client IPs and the `Host` header to the right port and nothing else, so no identity rule is re-implemented in the delivery layer.

## Outcome

- Every operation of the Auth, Sessions, Identity, Devices, Account and Admin rows of `api_v1_surface.md` is registered with its auth class, body class, state-changing flag, owner port method and ID parameters, and calls exactly one port method.
- Cookies are set exactly as 01's DTO gives them; redirects go to the return path or the SPA error route as 01 decides.
- Each handler module stays under 500 lines.

## Scope

### Included
- Auth: providers list, login attempt (201 with `authorization_url`, browser-binding cookie), provider callback (302), mobile code exchange, refresh.
- Sessions (list, revoke by id, revoke current), identity-link attempt, device grant start, device-token poll, approval lookup by `user_code`, decision, device list, revoke by id, `DELETE /devices/me`.
- Account: own view (with root node id), deletion request (202 with deadline), own quota; admin users list, update, quota, health; admin invites create (token shown once), list, revoke.
- Passing the client IP (trusted-proxy aware, E-21/c04) to `DeviceGrantService::request` and `lookup_for_approval`, and the request `Host` to sign-in calls.

### Excluded
- Identity rules, token rotation, rate counting (01, 07); the provider mock and end-to-end sign-in (pipeline AZ-3664); the expected scope values per operation (E-15/c13).
- Share, tree, transfer and media routes (E-21/c13, c14).

## Acceptance Criteria

**AC-1: One port call per operation**
Given a recording stub for every port of these groups
When each operation of the groups is called once with valid input and the matching credential
Then exactly one call reaches the named port method, the account passed is the principal's (never an id from the path, query or body), and the answer is 201 for the login attempt, 302 for the callback, 202 for the deletion request, and 200 with a body or 204 without one elsewhere

**AC-2: Sign-in begin and callback carry binding and Host**
Given a login attempt with provider, `return_to` and an invite token in the body, and then the provider callback with the browser-binding cookie
When both are called with `Host: sunnycloud.test`
Then `begin` receives the body fields and the host, the answer sets the browser-binding cookie and returns `authorization_url`; `complete` receives the provider, callback parameters, binding and host; a `Session` outcome sets the session cookie and redirects to `return_to`; an `UntrustedState` or `TrustedFailure` redirects to the SPA error route

**AC-3: Cookies come from the DTO (NFT-SEC-11)**
Given a stub sign-in outcome carrying a session cookie DTO
When the callback answers
Then `Set-Cookie` has the `__Host-` name, `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/` and no `Domain`, exactly as the DTO states; the server adds no attribute of its own

**AC-4: Device grant flow mapping**
Given a trusted proxy in front and stubs for `request`, `poll`, `lookup_for_approval` and `decide`
When the daemon starts a grant, polls while pending, after `SlowDown`, after denial and after expiry, and a session looks up and approves the grant
Then `request` and `lookup_for_approval` receive the client IP from `X-Forwarded-For`, the start answer carries `device_code`, `user_code`, `interval` and `expires_in`, and the four poll outcomes give four distinct machine-readable codes

**AC-5: Abuse limits map to 429 (AC-20)**
Given `begin`, `complete`, `lookup_for_approval` returning `RateLimited{retry_after: 30}`
When each route is called
Then each answers 429 with `Retry-After: 30`

**AC-6: Own-device and own-session operations**
Given a device token and a session
When the device token calls `DELETE /api/v1/devices/me` and the session calls `DELETE /api/v1/sessions/current`
Then `revoke_self` receives the device principal and `SessionService::revoke` receives the caller's own session id

**AC-7: Account and admin outcomes**
Given stubs returning `LastAdmin` for `update_user` and for the deletion request, `NotFound` for an unknown user, and a created invite
When the admin routes and `DELETE /api/v1/accounts/me` are called
Then `LastAdmin` gives 409, `NotFound` gives 404, the invite answer shows the plaintext token once and the invite list never shows a token

## Non-Functional Requirements

**Security**
- No token, code, `return_to` value or email reaches a log line.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Registry rows of these groups | Class, body class, flag, owner and ID parameters declared |
| AC-3 | Cookie rendering from the DTO | Attributes copied, none added |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S, recording stubs | Every operation once | One port call each; principal's account | AC-59 |
| AC-2 | Mode S | Begin and callback | Binding, host, cookie, redirects | AC-56, FT-P-62, FT-N-23 |
| AC-3 | Mode S | Cookie attributes (IT-11-20, cookie part) | Exactly as the DTO | NFT-SEC-11 step 3, NFT-SEC-10 (cookie parts) |
| AC-4 | Mode S, trusted proxy | Device grant mapping | IPs passed; four poll codes | AC-57, FT-N-45 |
| AC-5 | Mode S | Rate-limit mapping | 429 with `Retry-After` | AC-20, NFT-SEC-04, NFT-SEC-05 |
| AC-6 | Mode S | Own device and session | Right principal and id | AC-21 |
| AC-7 | Mode S | Account and admin outcomes | 409, 404, token once | FT-N-46, FT-P-65 |

## Constraints

- Paths and scopes follow 01 §3 and 07 §4 (the owners are authoritative, 11 §8).
- Handlers hold no identity rule; every decision is the port's answer.

## Risks & Mitigation

**Risk 1: Task at the 5-point cap (about 26 operations)**
- *Risk*: breadth.
- *Mitigation*: every handler is bind, call, map; the vertical slice of E-21/c13 goes first.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| RFC 8628 poll codes (`authorization_pending`, `slow_down`, `access_denied`, `expired_token`) have no status or body shape in helper 02 (problem body vs RFC 8628 `error` field) | E-15, E-02 owners | open | Medium |
| `DeviceOnly` for `DELETE /devices/me` vs 01's `SessionOrDevice` with the refusal in `revoke_self` | E-15, E-21 owners | open | Medium |
| `GET /api/v1/setup` (11 §8) is not in 01's route table; `list_providers` already returns `setup_required` | E-15, E-21 owners | open | Low |
| The route that completes a provider link (`complete_link`) is not listed by 01 | E-15 owner | open | Low |
| `GET /api/v1/accounts/me` is `SessionOrDevice` in 07 but "Session" in 11 §8; this task follows 07 | E-17, E-21 owners | open | Low |
| How web and mobile tokens are delivered after code exchange and refresh (body or cookie) is not stated | E-15 owner | open | Low |

## Contract

This task implements the Auth, Sessions, Identity, Devices, Account and Admin rows of the contract at `_docs/02_document/contracts/11_http_api/api_v1_surface.md` (producer E-21/c11).
Consumers MUST read that file — not this task spec — to discover the interface.
