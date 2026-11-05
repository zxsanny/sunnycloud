# Vault routes in the E-21 registry and OpenAPI, and VaultService plus hook in the E-22 composition root

**Task**: AZ-3926_vault_routes_registration
**Name**: Vault routes and wiring
**Description**: Register the four `/api/v1/vaults` operations (create, read, replace header, report unlock failure) with their auth classes, `ETag`/`If-Match` rules and 412/428 answers, and build `VaultService` and 20's erase hook in the server's composition root for v1.2 builds only
**Complexity**: 3 points
**Dependencies**: AZ-3923_vault_header_service, AZ-3924_vault_unlock_failure_report, AZ-3925_vault_erase_hook, AZ-3806_auth_extraction, AZ-3808_csrf_origin_check, AZ-3810_error_mapper, AZ-3827_route_registry_openapi, AZ-3834_csrf_idor_matrices, AZ-3789_composition_root, AZ-3764_sc_error
**Component**: 20 vault_service [v1.2] (routes served by 11, wired by 12)
**Tracker**: AZ-3926
**Epic**: AZ-3648 (E-30)
**Ref**: E-30/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — registry declarations, auth and body classes, CSRF rule (E-21/c11, c07, c08).
- `_docs/02_document/contracts/11_http_api/problem_details.md`, `_docs/02_document/contracts/shared_error/sc_error.md` — error body and the status table rows this task adds (E-21/c09, E-02/c01).
- `_docs/02_document/contracts/11_http_api/http_server.md` — the optional `VaultService` slot (E-21).
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md`, `part3_blob_share_media_vault.md` — `AccountLifecycleHook` and `VaultService` (E-00/c03, c05).

## Problem

11 reserves `/api/v1/vaults/**` for v1.2 (E-21/c11) and holds an optional `VaultService` slot, absent in v1.0 and v1.1 builds. Until the operations are registered, clients have no route, the generated two-account and CSRF matrices cannot cover them (AC-58, AC-59), and 07's purger never calls 20's hook. The HTTP side also owns two answers the port does not: 428 when `If-Match` is missing and 412 when it does not match. Several statuses and the header's wire encoding are not specified (see the contract's open points).

## Outcome

- In a v1.2 build the four operations are routed, documented in the generated OpenAPI with 201/200/404/412/428 and the problem schema, and covered by the generated security matrices.
- The routes follow the contract `vault_http_api.md`, whose open points are closed by this task.
- In a v1.2 build `VaultService` fills 11's slot and 20's hook is in 12's hook list; v1.0 and v1.1 builds stay free of both.

## Scope

### Included
- Four registry entries (method, path, owner port method, auth class `SessionOnly` for create and replace and `Session` for read and report, body class, state-changing flag, `x-id-params`).
- Thin handlers: bind, one `VaultService` call, map the result; `ETag` response header on create, read and replace; `If-Match` required on replace (428 when missing, 412 on mismatch); request body of the unlock-failure report ignored.
- The `sc_error` rows for the 412 and 428 codes and for the statuses left open in the contract (added with the E-02 owner).
- Closing the contract's open points (header wire encoding and body class, ETag string form, open statuses, report success status) and freezing the contract.
- Composition root (v1.2 builds): build `VaultService`, fill 11's slot, place 20's hook in 12's hook list at the position agreed for the root foreign key (E-30/c02 concern).

### Excluded
- The service logic (E-30/c03, c04, c05) and the membership wiring (E-30/c07).
- The matrix generators and the registry mechanism (E-21/c18, E-21/c11).
- The v1.0/v1.1 absence check itself (E-22/c03 AC-6), which this task must keep passing.

## Acceptance Criteria

**AC-1: Operations documented (AC-59, AC-64)**
Given a v1.2 server build
When the generated OpenAPI document is read
Then it lists the four operations with method, path, auth class, body class, state-changing flag, owner port method and `x-id-params`, documents 201, 200, 404, 412, 428 and the problem schema, and passes the router parity test and the naming lint of E-21/c11

**AC-2: ETag and If-Match over HTTP (AC-61, IT-20-2)**
Given a vault created with header H0
When the client sends PUT without `If-Match`, PUT with a stale ETag, and PUT with the current ETag and header H1
Then the first answers 428 and the second 412, both leaving the stored header byte-equal to H0, and the third answers 200 with an `ETag` derived from SHA-256 of H1; create and GET return the `ETag` header

**AC-3: Auth classes and CSRF (AC-57, AC-58)**
Given account A's vault and A's device token and session cookie
When the device token calls POST `/vaults` and PUT `.../header`, no credential calls any vault route, and a cross-origin PUT carries A's cookie
Then the device token gets 403 on both, the anonymous calls get 401, and the cross-origin PUT gets 403 `csrf_rejected`

**AC-4: Matrix coverage (AC-59)**
Given the generated two-account and CSRF matrices
When CI runs them, and a registry fixture drops one vault operation from the matrix input
Then account B gets 404 on all four operations with A's vault id (with and without A's `If-Match`), and the dropped-operation fixture fails CI

**AC-5: Report body is never kept (ST-20-2)**
Given the unlock-failure route
When a request carries a passphrase-shaped string and a 64-byte key-shaped value in its body
Then the answer is the success status, the audit row holds ids and time only, and the body text appears in no row and no log line

**AC-6: Errors mapped once**
Given each `VaultService` error (`NotFound`, `HeaderTooLarge`, `PreconditionFailed`, and `VaultExists` and the root errors as decided)
When it reaches the error mapper
Then each maps to one problem code and the status of its `sc_error` row, and no detail carries header bytes, ids of other accounts or key material

**AC-7: Wiring in v1.2 builds only (ADR-016)**
Given a v1.2 build and a v1.0 build
When each server starts
Then the v1.2 build has `VaultService` in 11's slot and 20's hook in 12's list at the agreed position, and the v1.0 build still passes E-22/c03 AC-6 (no vault hook, service or route)

## Non-Functional Requirements

**Compatibility**
- After v1.2 ships, the vault operations fall under the additive-only diff gate (AC-64).

**Reliability**
- Handlers hold no database transaction; each is one port call.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | `If-Match` extraction: absent, empty, malformed, present | Absent gives the 428 path; others reach the port |
| AC-6 | Error-to-problem mapping per variant | One code and status each |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | v1.2 build | OpenAPI document, parity test, naming lint | Four operations fully declared; tests pass | AC-64 |
| AC-2 | Vault with H0 (real API, PostgreSQL 18) | PUT without, stale and current `If-Match` | 428, 412, 200; H0 kept on refusals | IT-20-2, AC-61 |
| AC-3 | A's device token and cookie | SessionOnly routes, anonymous, cross-origin PUT | 403, 401, 403 `csrf_rejected` | ST-20-1, NFT-SEC-07 step 7 |
| AC-4 | Accounts A and B | Generated matrices; fixture without one operation | B 404 on all four; fixture fails | ST-20-1, NFT-SEC-13 |
| AC-5 | Vault of A | Report with secret-shaped body | Success; 0 secret matches in rows and logs | ST-20-2 |
| AC-7 | v1.2 and v1.0 builds | Start each; inspect slot and hook list | Present in v1.2 only | FT-P-73 |

## Constraints

- Thin delivery: bind, one business call, map the result (ADR-015); no business rule in the handlers.
- REST naming as in `api_v1_surface.md`; paths are the reserved ones of 20 §4.

## Risks & Mitigation

**Risk 1: Header too large for the default body class**
- *Risk*: the `Small` class (64 KiB) may not match the header cap or the chosen encoding.
- *Mitigation*: the body class is fixed together with Open point 1 of the contract and the E-30/c01 cap.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Open points 1-5 of `vault_http_api.md` (header encoding, ETag form, open statuses, report success status, missing `sc_error` rows) are unspecified in 20 §4 | E-30 / E-21 / E-02 owners | open | Medium |
| The mechanism that keeps 20 out of v1.0 and v1.1 builds is open (module-layout exception 6; E-22/c03 concern); AC-7 states only the observable result | E-22, E-30 owners | open | Medium |
| The hook position in 12's list depends on the foreign-key decision of E-30/c02 | E-30 / E-22 owners | open | High |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/20_vault_service/vault_http_api.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
