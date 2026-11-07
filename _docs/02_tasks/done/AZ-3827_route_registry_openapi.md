# Route registry, router, OpenAPI document, parity test and additive-only diff gate

**Task**: AZ-3827_route_registry_openapi
**Name**: Route registry and OpenAPI
**Description**: The single registry from which the public and internal routers, the OpenAPI document and the test-matrix inputs are generated, with the router/OpenAPI parity test, the naming rules, SPA serving with fallback and the CI gate that rejects removed or tightened operations
**Complexity**: 5 points
**Dependencies**: AZ-3801_crate_skeleton_accept_loop, AZ-3810_error_mapper, AZ-3761_ci_skeleton
**Component**: 11 http_api
**Tracker**: AZ-3827
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/http_server.md` — `openapi()` and the listeners (E-21/c03).
- `_docs/02_document/contracts/11_http_api/problem_details.md` — the error schema in the document (E-21/c09).

## Problem

About 60 operations are owned by eight components. If the router, the OpenAPI document and the security matrices are kept by hand they drift: an undocumented route escapes the CSRF and two-account matrices (AC-58, AC-59), a client generated from a stale document breaks, and a silently removed field breaks old clients (AC-64). Each operation also needs one declared auth class, body class and state-changing flag for the middleware to act on.

## Outcome

- One registry entry per operation: method, path template, owner port method, auth class, body class, every ID parameter (`x-id-params`), state-changing flag, and listener (public or internal).
- The routers and the OpenAPI document are generated from the registry; the document is served at `GET /api/v1/openapi.json` and returned by `HttpServer::openapi()` byte for byte.
- CI fails on an undocumented route, a phantom operation, a missing declaration, a naming violation, or a removal or tightening against the last release tag.
- Non-`/api` paths fall back to the bundled web app's index.

## Scope

### Included
- The registry mechanism and the declaration types (auth classes `Public`, `Session`, `SessionOrDevice`, `DeviceOnly`, `SessionOnly`, `Admin`, `ShareGrant`, `Worker`; body classes `Small`, `Commit`, `Batch`, `Block`, `Staged`, `Query`, `Auth`, `WorkerOutput`, `None`) that E-21/c05 and E-21/c07 act on.
- System routes `GET /api/v1/server-info` (route only; body by E-21/c06) and `GET /api/v1/openapi.json`; the reserved `/api/v1/vaults/**` group (registered by E-30/c06 in v1.2).
- OpenAPI generation (`utoipa`, version pinned at implementation), the four security schemes (cookie session, bearer session, bearer device, bearer worker), operation extensions for the declarations, and the problem schema.
- SPA static serving from the embedded bundle and the SPA fallback for non-`/api` paths.
- The parity and declaration test, the naming lint, and the additive-only diff tool wired into CI against the last release tag.
- Recording the extension names in `api_v1_surface.md`.

### Excluded
- The operations of each group (E-21/c12..c15 register them); the CSRF and IDOR generators (E-21/c18); the scope value check of identity operations (E-15/c13).
- Middleware behaviour per class (E-21/c05, c07, c08).

## Acceptance Criteria

**AC-1: Router and document agree (AC-59, AC-64)**
Given the live public and internal route tables and `HttpServer::openapi()`
When the parity test compares the sets of (method, path template)
Then they are equal, with 0 undocumented routes and 0 phantom operations, and `GET /api/v1/openapi.json` returns the same bytes as `openapi()`

**AC-2: Every operation is fully declared**
Given the generated document
When every operation is read
Then each carries one auth class, one body class, the state-changing flag, the owner port method and `x-id-params` naming every ID parameter in path, query, body or header; a test route registered without one of them fails the test

**AC-3: Naming rules hold (FT-P-70)**
Given every path in the document
When the naming lint runs
Then every API path starts with `/api/v1`, uses plural lowercase kebab nouns, has no trailing slash, puts filters in the query, uses the colon form only for `commits:batch` and `revisions:lookup`, and no route exists outside `/api` except ops, test-clock and the SPA fallback; the document passes an OpenAPI 3.x validator with 0 errors

**AC-4: Additive-only diff gate (AC-64)**
Given the last release document as a fixture and modified copies with an operation removed, a required request field added, an enum value removed, a response field removed, a new optional field and a new operation
When the diff tool runs on each
Then the first four fail and the last two pass, and the CI job runs the tool against the last release tag

**AC-5: SPA fallback**
Given the embedded web bundle
When a non-`/api` path such as `/files/a/b` is requested, and an unknown `/api/v1/...` path is requested
Then the first returns the bundle's index and the second a 404 problem body (never the index)

**AC-6: Listener placement is a registry fact (AC-79)**
Given a route declared for the internal listener
When the routers are generated
Then it appears only in the internal router and in the document with the worker security scheme, and never in the public router

## Non-Functional Requirements

**Performance**
- Routing adds no measurable latency beyond PT-11-2's `server-info` p95 of 2 ms (failure 10 ms), measured in E-21/c20.

**Compatibility**
- The document is OpenAPI 3.x and consumable by the web client generator (E-26/c06) and the sync-core client (E-23).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Registry entry missing a declaration | Fails at build or in the test |
| AC-3 | Naming lint over valid and invalid sample paths | Valid pass; trailing slash, verb form, camelCase fail |
| AC-4 | Diff tool over the six mutations | Four fail, two pass |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S, all registered routes | Router and OpenAPI parity (IT-11-11) | Equal sets; served bytes equal | AC-59 |
| AC-3 | Generated document | Validation and naming (IT-11-11) | 0 errors; rules hold | FT-P-70 |
| AC-4 | Release fixture and mutations | Diff gate (IT-11-12) | First four fail, last two pass | AC-64 |
| AC-5 | Mode S with the bundle | SPA fallback (IT-11-11) | Index for SPA paths; 404 for unknown `/api` | |
| AC-6 | Mode S | Internal route placement | Absent from the public router | AC-79 |

## Constraints

- The owning component's spec is authoritative for its paths (11 §8); the registry follows the owners' route tables, not 11's indicative paths.
- No route is added to a router except through the registry.

## Risks & Mitigation

**Risk 1: Registry drifts from handlers**
- *Risk*: a handler wired outside the registry.
- *Mitigation*: AC-1 parity in CI on every change.

**Risk 2: Task at the 5-point cap**
- *Risk*: generation, lint, diff tool and SPA serving overrun.
- *Mitigation*: the matrices and the identity scope values stay in E-21/c18 and E-15/c13.

## Runtime Completeness

- Named capability: OpenAPI generation from the code (`utoipa`) and the additive-only diff.
- Production code that must exist: the generated document served by the running server and printed by `openapi()`; a diff tool that compares two documents structurally.
- Unacceptable substitutes: a hand-maintained OpenAPI file; a text diff of the document.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Extension names for auth class, body class, state-changing flag and owner port method are not given (only `x-id-params`); this task fixes them and records them in the contract | E-21 owner | open | Low |
| Whether the internal router belongs in the same document is inferred from the "bearer worker" security scheme in 11 §9 | E-21 owner | open | Low |
| `revisions:lookup` (up to 1,000 hashes, about 70 KB) has no body class; `Small` (64 KiB) is too small | E-13, E-21 owners | open | Medium |
| The GET provider callback changes state, which the "GET never mutates" check must exempt by name | E-15, E-21 owners | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/11_http_api/api_v1_surface.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
