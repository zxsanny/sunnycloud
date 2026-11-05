# Transfer documentation: error codes, one-process note, client retry rules

**Task**: AZ-3762_transfer_docs
**Name**: Transfer documentation
**Description**: Document every transfer error code with its status and the client's action, state the one-API-process rule for operators, and write the client retry rules for sessions, blocks, batches and conflict names
**Complexity**: 3 points
**Dependencies**: AZ-3731_transfer_spike_resend_stale_base, AZ-3732_transfer_spike_nothing_to_send, AZ-3754_transfer_commit_batch, AZ-3755_transfer_download, AZ-3756_transfer_janitors_reservations, AZ-3827_route_registry_openapi, AZ-3829_tree_transfer_feed_handlers
**Component**: 03_transfer
**Tracker**: AZ-3762
**Epic**: AZ-3643 (E-14)
**Ref**: E-14/c16

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/03_transfer/upload_service.md`, `commit_service.md`, `download_service.md` — the behaviour being documented.

## Problem

Three clients (web, desktop, Android) and later iOS implement the transfer protocol. They need one place that says what each error means and what to do next: retry after `Retry-After`, resend blocks, resume by `client_key`, persist a batch `commit_id`, never retry a taken name. Operators must know that v1.0 runs exactly one API process because upload progress is in memory (decision 12, R06). The OpenAPI document must list the same codes.

## Outcome

- A transfer error catalogue that matches the typed errors one to one, and the same codes on the transfer routes of the generated OpenAPI document.
- An operator note on the one-process rule.
- Client retry rules that match the contracts and the E-14/c01 and E-14/c02 decisions.

## Scope

### Included
- Catalogue: each code of 03 §4 with status, meaning and client action.
- Check that the generated OpenAPI (E-21/c11) lists the codes on `POST /api/v1/uploads`, `GET|DELETE /api/v1/uploads/{upload_id}`, `PUT /api/v1/uploads/{upload_id}/blocks/{index}`, `PUT /api/v1/staged-uploads/{staged_key}`, `POST /api/v1/commits`, `POST /api/v1/commits:batch` and the content routes.
- Operator note: one API process, no replicas for uploads; check that the release compose file sets no `replicas` for the API.
- Client rules: `client_key` forms (`web:`, `desk:`, `asset:`) and when a new key starts a fresh session (AC-50); `need_blocks` resume; `Overloaded` with `Retry-After`; batch retry (persist `commit_id`, staged keys and entries before the first send, reuse unchanged; re-send failed entries under a new id, FT-N-38); `NothingToSend` healing for the large path; server-assigned conflict-copy names (no retry on a taken name); the stale-base response per E-14/c01.

### Excluded
- Writing the OpenAPI generator or route annotations (E-21/c11, E-21/c13); client implementations (E-23, E-26, E-27).

## Acceptance Criteria

**AC-1: Catalogue matches the errors**
Given the transfer error table
When the catalogue is compared with it
Then every code appears once with status, meaning and client action, and no code is missing or extra

**AC-2: OpenAPI lists the codes**
Given the generated OpenAPI document
When its transfer routes are checked against the catalogue
Then each route lists the codes that route can return, with the catalogue's status

**AC-3: One-process rule stated**
Given the operator deployment documentation and the release compose file
When they are reviewed
Then the documentation states one API process and that replicas are unsupported for uploads, and the compose file sets no `replicas` for the API (a difference is reported to E-00)

**AC-4: Client retry rules complete**
Given the upload, commit and download contracts and the E-14/c01 and E-14/c02 decisions
When the client rules are reviewed against them
Then each rule in Scope is present, matches the contracts, and names the test that proves it (FT-N-38, FT-P-67, IT-03-4, IT-03-7)

## Non-Functional Requirements

**Compatibility**
- Codes are stable; additions only (OpenAPI additive-only diff, E-21/c11).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Catalogue against the error table | One-to-one |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | Generated OpenAPI | Route codes vs catalogue | Match | FT-P-70 |
| AC-3 | Deployment docs, compose file | Review | One process; no replicas | |
| AC-4 | Contracts, decisions | Rules review | Complete and consistent | FT-N-38, FT-P-67 |

## Constraints

- Documentation only; no change to code or the compose file in this task.

## Risks & Mitigation

**Risk 1: Docs drift from code**
- *Risk*: a new error code is added without a catalogue entry.
- *Mitigation*: AC-1's comparison runs in CI.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `module-layout.md` names no path or owner for operator and client-protocol documentation beyond `README.md` (E-00) | E-00 / owner | open | Low |
| 409 vs 412 for `need_blocks` and `edited_since` is open (E-21/c02, G-11-1) | E-21 | open | Low |
