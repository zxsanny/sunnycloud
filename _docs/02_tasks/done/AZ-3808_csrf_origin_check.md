# CSRF origin check

**Task**: AZ-3808_csrf_origin_check
**Name**: CSRF origin check
**Description**: Refuse every cookie-authenticated POST, PUT, PATCH or DELETE that does not prove same origin (`Sec-Fetch-Site: same-origin` or `Origin` equal to `public_url`) with 403 `csrf_rejected`, exempt bearer requests, and keep GET and HEAD free of state changes
**Complexity**: 3 points
**Dependencies**: AZ-3806_auth_extraction, AZ-3810_error_mapper, AZ-3827_route_registry_openapi
**Component**: 11 http_api
**Tracker**: AZ-3808
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c08

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — CSRF rule and state-changing flags (E-21/c11).
- `_docs/02_document/contracts/11_http_api/problem_details.md` — the 403 body (E-21/c09).

## Problem

The web app authenticates with a cookie, so a page on another site can make the victim's browser send a mutation with that cookie. `SameSite=Lax` alone does not cover sibling subdomains or every request type. AC-58 requires 403 on every state-changing operation for a cross-origin request and for one that carries neither `Origin` nor `Sec-Fetch-Site`.

## Outcome

- Layer 9 of 11 §5: for cookie-authenticated requests on state-changing methods, only `Sec-Fetch-Site: same-origin` or `Origin` equal to the configured public origin passes; everything else is 403 `csrf_rejected`.
- Bearer-authenticated requests are exempt; GET and HEAD are not checked and never change state.

## Scope

### Included
- The rule, its position after auth extraction (it needs to know whether the credential came from a cookie), the origin comparison against `public_url` (scheme, host and port).
- The `csrf_rejected` code (with its `sc_error` row through E-02 if missing).
- A test over representative routes of each group; the generated N-of-N matrix is E-21/c18.

### Excluded
- The matrix generated from OpenAPI (E-21/c18) and its Mode R run (E-21/c20, ST-11-1).
- Refresh-token races (01, NFT-SEC-10).

## Acceptance Criteria

**AC-1: Foreign or missing origin is refused (AC-58)**
Given a valid session cookie and one state-changing route of each method (POST, PUT, PATCH, DELETE)
When each is called with `Origin` foreign, a same-site sibling subdomain, `null`; with `Sec-Fetch-Site` cross-site, same-site, none; and with neither header
Then every call answers 403 with code `csrf_rejected` and the stub port is not called

**AC-2: Same origin passes**
Given the same routes and cookie
When each is called with `Origin: https://sunnycloud.test` (the configured public URL) or with `Sec-Fetch-Site: same-origin`
Then the request passes the layer and the stub answers 2xx

**AC-3: Bearer requests are exempt**
Given a session bearer and a device bearer
When they call the same routes with a foreign `Origin` and with no origin headers
Then the requests pass the layer

**AC-4: Origin comparison is exact**
Given `public_url = https://sunnycloud.test`
When `Origin` is `http://sunnycloud.test`, `https://sunnycloud.test:8443` and `https://sunnycloud.test.evil.example`
Then all three are refused

**AC-5: GET and HEAD change nothing**
Given every GET and HEAD route of the registry with a stub state counter
When each is called with cross-site headers and a cookie
Then no stub state counter changes (the provider callback is exempted by name, see Flagged concerns)

## Non-Functional Requirements

**Security**
- Coverage of this layer at least 90% (quality thresholds, security-critical).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1, AC-2 | Decision table over (credential source, method, `Origin`, `Sec-Fetch-Site`) | Pass only for the two same-origin proofs or a bearer |
| AC-4 | Origin normalisation and comparison | Exact scheme, host, port match |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S, cookie | Foreign and missing origins (IT-11-6, layer part) | 403 `csrf_rejected` | AC-58, NFT-SEC-09 |
| AC-2 | Mode S, cookie | Same origin (IT-11-6) | 2xx | NFT-SEC-09 step 3 |
| AC-3 | Mode S, bearer | Exemption (IT-11-6) | Passes | NFT-SEC-09 step 4 |
| AC-4 | Mode S | Exact comparison | As listed | |
| AC-5 | Mode S, all GET/HEAD | No mutation (IT-11-6) | Counters unchanged | NFT-SEC-09 step 5 |

## Constraints

- The public origin comes from `public_url` only, never from the `Host` header.

## Risks & Mitigation

**Risk 1: A new mutation route escapes**
- *Risk*: a route registered with the wrong state-changing flag.
- *Mitigation*: the method decides the check (every POST, PUT, PATCH, DELETE), not the flag; E-21/c18 generates N of N from the registry.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The GET provider callback consumes state and creates a session; "GET never mutates" must exempt it explicitly | E-15, E-21 owners | open | Low |
| Whether a refresh request from the web is cookie-authenticated (and so under CSRF) is not stated by 01 | E-15 owner | open | Low |

## Contract

This task implements the CSRF row of the contract at `_docs/02_document/contracts/11_http_api/api_v1_surface.md` (producer E-21/c11).
Consumers MUST read that file — not this task spec — to discover the interface.
