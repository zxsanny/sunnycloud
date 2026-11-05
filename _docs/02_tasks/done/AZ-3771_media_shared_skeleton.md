# media-shared skeleton: job DTOs, policy types, decoder protocol types

**Task**: AZ-3771_media_shared_skeleton
**Name**: media-shared skeleton
**Description**: The plain data types shared by the API side (08), the supervisor (09) and the decoder (10): worker job DTOs, transcode policy types and decoder protocol messages, with no sqlx, no I/O and no behaviour
**Complexity**: 2 points
**Dependencies**: AZ-3766_sc_ids, AZ-3652_initial_structure
**Component**: media-shared (types for 08, 09, 10)
**Tracker**: AZ-3771
**Epic**: AZ-3621 (E-00)
**Ref**: E-00/c06

## Document Dependencies

- `_docs/02_document/contracts/shared_ids_hashing/sc_ids.md` — `FileHash` in `ClaimedJob`, produced by E-04/c01.

## Problem

The trusted supervisor and the untrusted decoder must agree on every job and protocol message, and neither may link server code (ADR-009, RESTRICT-13). E-18, E-19 and E-20 run in parallel later; without one crate of shared types each would define its own, and a field drift between a hostile-input parser and its producer is a security bug.

## Outcome

- One definition of each job DTO, policy type and decoder message, serialisable to JSON.
- Decoder protocol types reject unknown fields.
- The crate cannot depend on sqlx, I/O, `contracts` or a component (crate-graph test).

## Scope

### Included
- Module `types`: `Lane`, `JobKind`, `ClaimedJob`, lease, output metadata, skip outcome, failure disposition and report, failure result, `WorkerStatus`, `DerivedKind`, with the serialised names of the worker routes (08 §5).
- Module `policy`: `Probe`, `Plan`, `Decision`, `DecoderCapabilities`, `EncoderPref`, `Params`, `OutputSpec` as types.
- Module `decoder_protocol`: request, response, event and `DecodeError` types of decoder protocol v1 (10 §3), rejecting unknown fields.
- JSON round-trip tests for every type and the wire-name tests.

### Excluded
- `is_compliant`, `decide`, `poster_offset_ms` and the `Params` values (E-18); framing, the 64 KiB frame cap and numeric range checks (E-19); the HTTP client and socket client (E-20).

## Acceptance Criteria

**AC-1: Every type round-trips**
Given one value of every type in the three modules
When each is serialised to JSON and parsed back
Then the parsed value equals the original

**AC-2: Decoder messages reject unknown fields (10 §3)**
Given a decoder reply with one extra field
When it is parsed
Then parsing fails

**AC-3: Wire names match the worker routes**
Given `Lane::Image`, `SkipOutcome::SkippedCompliant`, `Disposition::Permanent` and `FailResult::Dead`
When they are serialised
Then they read `"image"`, `"skipped_compliant"`, `"permanent"` and `"dead"`

**AC-4: No forbidden dependency (ADR-009)**
Given the crate
When the AZ-3652 crate-graph test runs and its dependency list is read
Then it has no sqlx, no HTTP client, no `contracts`, no component crate and no async runtime I/O

**AC-5: Plans carry no free text**
Given the `Plan` type
When its fields are listed
Then every field is an enumeration, a boolean or a bounded number; none is a string passed to an encoder

## Non-Functional Requirements

**Compatibility**
- serde-level dependencies and `sc_ids` only; builds with the workspace MSRV and `--locked`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | JSON round trip of every type | Equal |
| AC-2 | Extra field in a decoder reply | Rejected |
| AC-3 | Serialised names | As listed |
| AC-4 | Crate-graph test | Passes |
| AC-5 | Field types of `Plan` | No free string |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| — | None: no runtime behaviour; exercised by the media pipeline tasks (E-22/p08, p09) through E-18..E-20 | — | — | — |

## Constraints

- ADR-009 (types only, no queue module), ADR-010; `decoder_protocol` belongs to 10, `types` and `policy` to 08 for later changes (module-layout).

## Risks & Mitigation

**Risk 1: API-side copy of the job DTOs drifts**
- *Risk*: `contracts` part 3 declares the same shapes for 11.
- *Mitigation*: identical serialised names fixed in both contracts; E-18/E-20 wire test.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The JSON encoding of `content_hash` in `ClaimedJob` (hex or base64) is not specified by 08 or 09. | E-18 owner | open | Medium |
| Integer widths of `params_version`, `attempt`, `image_age_days` are not specified (contract chooses unsigned 32-bit). | E-18 owner | open | Low |
| `OutputSpec` and `EncoderPref` are named without fields; type names for some route bodies are introduced by the contract. | E-18 / E-20 owners | open | Low |
| Worker job DTOs also exist in `contracts` (part 3 concern 1). | Project owner | open | Medium |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/media-shared/skeleton_types.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
