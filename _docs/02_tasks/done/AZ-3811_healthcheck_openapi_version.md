# `healthcheck`, `openapi`, `version`

**Task**: AZ-3811_healthcheck_openapi_version
**Name**: Local commands
**Description**: The three commands that need no business call: the Compose health probe of `/readyz`, the OpenAPI document used by the CI diff, and the version line
**Complexity**: 2 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3790_per_command_config, AZ-3827_route_registry_openapi, AZ-3832_ops_endpoints_readiness
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3811
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c15

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — command rows (E-22/c02).

## Problem

The API image has no curl, so Compose needs `sunnycloud healthcheck` to probe readiness. CI diffs the OpenAPI document between releases (AC-64), so `openapi` must print the same bytes every time and work in a build job with no database. Operators and bug reports need the exact version.

## Outcome

- `healthcheck`: probes `127.0.0.1:9090/readyz`; exit 0 on 200, non-zero on 503 or when nothing answers.
- `openapi`: prints the document from 11's route registry; valid JSON; byte-identical across runs; equal to the served `GET /api/v1/openapi.json`; needs no database and no master key.
- `version`: prints the semver and the git commit; needs no configuration.

## Scope

### Included
- The three commands and their exit codes.

### Excluded
- The `/readyz` contents (E-21/c16 and the registered checks), the OpenAPI generation and diff tool (E-21/c11), the Compose healthcheck stanza (E-00/c11).

## Acceptance Criteria

**AC-1: Health probe (IT-12-18)**
Given a ready stack, then the database stopped so `/readyz` returns 503, then the API stopped
When `healthcheck` runs in each state
Then it exits 0, then non-zero, then non-zero

**AC-2: OpenAPI is stable and standalone (IT-12-18, FT-P-70, AC-64)**
Given no database and no master key
When `openapi` runs twice
Then both outputs are valid JSON, byte-identical, and equal to the document served at `GET /api/v1/openapi.json` by a running server of the same build

**AC-3: Version (IT-12-18)**
Given no configuration at all
When `version` runs
Then it prints a semver and the git commit and exits 0

## Non-Functional Requirements

**Performance**
- `healthcheck` returns within the Compose healthcheck timeout of E-00/c11.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Status mapping 200 / 503 / connection refused | 0 / non-zero / non-zero |
| AC-3 | Version string | Semver and commit present |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Ready stack; DB stopped; API stopped | `healthcheck` (IT-12-18) | 0; non-zero; non-zero | |
| AC-2 | No DB, no key; running server of the same build | `openapi` twice and the served document (IT-12-18) | Identical bytes; equal to served | FT-P-70, AC-64 |
| AC-3 | No config | `version` (IT-12-18) | Semver and commit | |

## Constraints

- `openapi` must not construct services that need a database.

## Risks & Mitigation

**Risk 1: Non-deterministic document**
- *Risk*: map ordering makes two runs differ and the CI diff flaps.
- *Mitigation*: AC-2 compares bytes across runs.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 11 lists `openapi()` as a method of `HttpServer`, which is built from `ApiServices`; AC-2 needs the document without constructed services | E-21 owner | open | Low |
| The probe timeout of `healthcheck` is not stated | E-22 owner | open | Low |
| G-15 (OpenAPI document URL) is E-21's; AC-2 uses `GET /api/v1/openapi.json` from 11 §8 | E-21 owner | open | Low |
