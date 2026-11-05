# decoder_protocol module: framing, strict parser, ranges and fuzz

**Task**: AZ-3852_decoder_protocol_module
**Name**: Decoder protocol module
**Description**: The shared frame codec and strict parser of decoder protocol v1 in `media-shared`, used by the runner and the supervisor, with the 64 KiB cap, range checks, `ProtocolViolation` and a fuzzed parser
**Complexity**: 3 points
**Dependencies**: AZ-3771_media_shared_skeleton, AZ-3764_sc_error, AZ-3652_initial_structure
**Component**: 10_decoder (`media-shared::decoder_protocol`, owned by 10)
**Tracker**: AZ-3852
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c04

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/media-shared/skeleton_types.md` — message types of decoder protocol v1, produced by E-00/c06.

## Problem

The supervisor and the decoder each assume the other is hostile (ADR-009). If each side wrote its own framing and validation, a field or range rule could drift and a hostile frame could be accepted on one side. E-00/c06 defined the message types only; the frame codec, the size cap, the range checks and the session rules still need one owner, and both E-19/c05 and E-20/c05 consume them.

## Outcome

- One codec and one strict parser serve both sides; neither has a lenient path.
- Every hostile frame class named by the tests is refused as `ProtocolViolation` without unbounded allocation.
- The open points of the contract are fixed and the contract moves to frozen.

## Scope

### Included
- The frame codec: 4-byte length prefix, UTF-8 JSON payload, payload at most 65,536 bytes, oversize refused before any payload byte is read.
- Strict parsing in both directions: unknown fields, unknown enumeration values, invalid UTF-8, malformed JSON, out-of-range numbers and free text in `Plan` are `ProtocolViolation`.
- The message envelope (request, response, event), the duration and timestamp encodings, and the per-field range table; written into the contract (open points 1-4).
- The session rules of the contract as a reusable state check (one job request in flight, `Busy`, `cancel`, repeated `hello`, out-of-sequence requests; open points 5-6).
- A fuzz target for the parser with its seed corpus kept with the tests.

### Excluded
- The socket server, clamping and dispatch (E-19/c05) and the client, reconnect and slot handling (E-20/c05).
- The message types themselves (E-00/c06) and the policy (E-18).

## Acceptance Criteria

**AC-1: Every message round-trips**
Given one value of every request, response, event and `DecodeError` variant
When each is framed and parsed back
Then the parsed value equals the original

**AC-2: Oversize frames are refused early (IT-10-3, IT-09-13)**
Given a payload of 65,537 bytes and a length prefix announcing 4 GiB
When each is read
Then both are `ProtocolViolation`, no payload byte of the 4 GiB frame is read, and no buffer above 64 KiB is allocated

**AC-3: Strict schema (IT-10-3, IT-09-13, ST-10-2)**
Given a message with an unknown field, one with an unknown media kind, one with invalid UTF-8, and a `transcode` whose `Plan` carries a string
When each is parsed
Then each is `ProtocolViolation`

**AC-4: Ranges (ST-10-2, IT-09-13)**
Given `render_still.max_edge` of -1, 0 and 100000, a reply `width` of 0 and of 4294967295, and a negative `duration_ms`
When each is parsed
Then each is `ProtocolViolation`, and the contract holds the full range table that decided it

**AC-5: Limits are clamped by the runner, not refused here (IT-10-3)**
Given `limits{cpu: 9999, memory_bytes: 1 TiB, wall: 1 day}` within their types
When the request is parsed
Then it is accepted unchanged

**AC-6: Session rules (IT-10-1, IT-10-3)**
Given a session state with a job request in flight
When a second job request, a `cancel`, and a repeated `hello` on an idle session arrive
Then the second job request yields `Busy`, `cancel` is accepted, the idle `hello` is accepted, and the out-of-sequence cases fixed in the contract yield `ProtocolViolation`

**AC-7: Parser survives fuzzing**
Given the fuzz target and its seed corpus (one valid frame per message kind plus the hostile cases above)
When the fuzz run of the CI budget completes
Then it reports no panic and no allocation above the 64 KiB cap

**AC-8: Partial frames return nothing**
Given a frame that announces 100 bytes and delivers 40
When the reader is polled
Then no message is returned and the reader can be abandoned by its caller without leaking the partial buffer

## Non-Functional Requirements

**Performance**
- Parsing a 64 KiB frame costs well under the supervisor's 40 ms per-job overhead budget (PT-09-1).

**Reliability**
- The module has no I/O of its own beyond reading and writing a byte stream given to it; it stays inside `media-shared`'s serde-level dependency rule.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Round trip of every message | Equal values |
| AC-2 | 65,537-byte payload; 4 GiB prefix | `ProtocolViolation`; no large allocation |
| AC-3 | Unknown field, unknown enum, bad UTF-8, free text in `Plan` | `ProtocolViolation` |
| AC-4 | Range-edge values | `ProtocolViolation` |
| AC-5 | Over-ceiling limits | Accepted |
| AC-6 | Session state table | `Busy` / accepted / violation per contract |
| AC-7 | Fuzz run | No panic, bounded memory |
| AC-8 | Partial frame | No message |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2, AC-3 | Runner in its image (E-19/c05) | IT-10-3 frame cases | Violation, runner exits | AC-25 |
| AC-3, AC-4 | Supervisor with the fake decoder (E-20/c11) | IT-09-13 hostile replies | Violation, slot unhealthy, reconnect | AC-25 |

## Constraints

- `media-shared` keeps its rule: serde-level crates only, no sqlx, no `contracts`, no component crate (crate-graph test).
- Only 10 edits the `decoder_protocol` module (module-layout).

## Risks & Mitigation

**Risk 1: A range bound rejects real media**
- *Risk*: an upper bound on width or duration refuses a genuine large photo or long clip.
- *Mitigation*: bounds come from the configured ceilings (`photo_max_pixels`, input limits, output limits) and are reviewed against DS-03 and DS-08 in the round-trip tests.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No spec gives the numeric ranges, the prefix byte order, the envelope tags, the duration encoding or the full sequence rule (contract open points 1-6); this task fixes them and freezes the contract | E-19 owner | open | Low |
| `render_still` and `Plan` carry no `params_version` or quality values, yet 10 §4 says WebP Q, CRF and preset come from `Params` (contract open point 7) | E-18 and E-00/c06 owners | open | Medium |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/10_decoder/decoder_protocol.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
