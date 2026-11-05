# Generated AC-58 CSRF matrix and AC-59 two-account IDOR matrix from the registry

**Task**: AZ-3834_csrf_idor_matrices
**Name**: CSRF and IDOR matrix generators
**Description**: Generate, from `HttpServer::openapi()`, one CSRF case per state-changing operation and header set and one two-account case per (operation, ID parameter); run both on the stub-services server; fail CI when an operation or ID parameter has no case; make both runnable against the real stack for E-21/c20
**Complexity**: 5 points
**Dependencies**: AZ-3808_csrf_origin_check, AZ-3827_route_registry_openapi, AZ-3828_identity_admin_handlers, AZ-3829_tree_transfer_feed_handlers, AZ-3830_share_media_handlers
**Component**: 11 http_api
**Tracker**: AZ-3834
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c18

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — operations, state-changing flags, `x-id-params`, auth classes (E-21/c11).
- `_docs/02_document/contracts/11_http_api/problem_details.md` — `csrf_rejected` 403 and the uniform 404 (E-21/c09).

## Problem

AC-58 and AC-59 are stated over every operation: 403 for every cross-origin cookie mutation, and 0 successful cross-account calls over every operation and every ID parameter, with CI failing when a new operation is not covered. With about 60 operations, hand-written lists fall behind the code (R09). The matrices must therefore be generated from the same registry that builds the router.

## Outcome

- A CSRF generator producing N cases (N printed) from every state-changing operation and the header sets of IT-11-6, plus bearer and GET/HEAD control cases.
- An IDOR generator producing one case per (operation, ID parameter) in path, query, body or header, using account B's credential with account A's id, compared with a random id.
- Both run on the Mode S server in CI and fail when an operation or ID parameter has no case; both take a base URL and credentials so E-21/c20 runs them on the real stack (ST-11-1, ST-11-3).

## Scope

### Included
- The two generators reading the generated OpenAPI document, their case data (valid request bodies per operation from the document's schemas), and the uncovered-operation check.
- Mode S runs with recording fakes that answer `NotFound` for ids owned by another account, and a state counter for GET/HEAD checks.
- The admin-only case: non-admin session on every `Admin` operation gives 403 (G-S6 as 07 and 11 state).

### Excluded
- Real-stack runs (E-21/c20: ST-11-1, ST-11-3; E-31: NFT-SEC-09, NFT-SEC-13); component-level IDOR suites (E-13/c15 and others).

## Acceptance Criteria

**AC-1: CSRF N of N (AC-58)**
Given the generated document and a valid session cookie
When every state-changing operation is called with `Origin` foreign, sibling subdomain and `null`, with `Sec-Fetch-Site` cross-site, same-site and none, and with neither header
Then every case answers 403 `csrf_rejected`, N (operations x header sets) is printed, and no stub state changed

**AC-2: CSRF controls pass**
Given the same operations
When each is called with `Origin` equal to the public URL, with `Sec-Fetch-Site: same-origin`, and with a bearer credential and a foreign origin
Then each passes the CSRF layer (the stub answers 2xx)

**AC-3: GET and HEAD never mutate**
Given every GET and HEAD operation with a cookie and cross-site headers
When each is called
Then no stub state counter changes, except the operations exempted by name (the provider callback)

**AC-4: Two-account matrix (AC-59)**
Given account A's ids for every resource kind the document names and account B's credential
When every (operation, ID parameter) is called by B with A's id, and again with a random id
Then every call answers 404, the two bodies of each pair are equal apart from `request_id`, and every port call received B's account; a request body carrying A's `account_id` is ignored (NFT-SEC-14 step 6)

**AC-5: Admin operations refuse non-admins**
Given a non-admin session
When every `Admin` operation is called
Then each answers 403 and no port call is made

**AC-6: Uncovered operations fail CI**
Given an extra test route registered with a state-changing flag and an ID parameter but no case data
When the generators run in CI mode
Then both fail, naming the route and the missing ID parameter; the matrix size equals the number of (operation, ID parameter) pairs in the registry

## Non-Functional Requirements

**Performance**
- Both Mode S matrices finish within 60 s (IT-11-6 budget).

**Security**
- Coverage of the CSRF and auth gating code at least 90% after these runs (quality thresholds).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-6 | Generator over a fixture document with a planted uncovered operation | Fails naming it |
| AC-4 | Extraction of ID parameters from path, query, body and header schemas | Every `x-id-params` entry found |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S, cookie, generated cases | CSRF matrix (IT-11-6) | 403 on N of N; state unchanged | AC-58, NFT-SEC-09 steps 1, 2, 6b |
| AC-2 | Mode S | Controls (IT-11-6) | Pass | NFT-SEC-09 steps 3, 4 |
| AC-3 | Mode S, state counters | GET and HEAD (IT-11-6) | No change | NFT-SEC-09 step 5 |
| AC-4 | Mode S, two accounts in fakes | IDOR matrix (IT-11-10, IT-11-11) | 404 uniform; B's account passed | AC-59, NFT-SEC-13, NFT-SEC-14 |
| AC-5 | Mode S | Admin operations | 403, no port call | NFT-SEC-13 step 4 |
| AC-6 | CI mode with a planted route | Uncovered operation | Both generators fail | NFT-SEC-09 step 7, NFT-SEC-13 step 5 |

## Constraints

- The generators read only the generated document; they keep no second list of operations.

## Risks & Mitigation

**Risk 1: Case data drifts from schemas**
- *Risk*: generated bodies fail validation, so a 400 hides a missing CSRF check.
- *Mitigation*: AC-2 controls must reach 2xx; a 400 in a control fails the run.

**Risk 2: Task at the 5-point cap**
- *Risk*: two generators plus body synthesis.
- *Mitigation*: real-stack runs and system scenarios stay in E-21/c20 and E-31.

## Runtime Completeness

- Named capability: registry-driven CSRF and two-account matrices (AC-58, AC-59).
- Production code that must exist: generators that read the server's generated document and fail on uncovered operations.
- Unacceptable substitutes: a hand-written list of operations; a matrix that skips operations whose bodies it cannot synthesise.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The GET provider callback changes state and needs a named exemption from the GET check | E-15, E-21 owners | open | Low |
| NFT-SEC-13 lists "job" among resource kinds; jobs are reachable only on the internal listener, so the user-facing matrix covers them only through thumbnail and transcode routes | E-21, E-31 owners | open | Low |
