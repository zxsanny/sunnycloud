# DecoderClient: strict protocol client, reconnect with backoff, hello and self-check gate

**Task**: AZ-3871_worker_decoder_client
**Name**: Worker DecoderClient
**Description**: The supervisor's client of decoder protocol v1 on each slot's socket: typed calls and errors, every reply treated as hostile, reconnect with backoff after a runner exit, and a start-up gate that lets a slot take jobs only after `hello` and an all-ok `self_check`
**Complexity**: 3 points
**Dependencies**: AZ-3868_worker_crate_skeleton, AZ-3852_decoder_protocol_module
**Component**: 09_worker_supervisor
**Tracker**: AZ-3871
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/10_decoder/decoder_protocol.md` — framing, parser, session rules and reaction to violations, produced by E-19/c04.

## Problem

The decoder is untrusted, so every frame it sends may be oversized, malformed or out of range (AC-25, R01). The runner exits after any hostile-class outcome or watchdog abort and Compose restarts it; the supervisor has no Docker socket and must notice, reconnect and re-prove isolation before it hands the decoder another user's file (R34).

## Outcome

- Typed `hello`, `self_check`, `probe`, `render_still`, `transcode`, `cancel` calls with typed `DecodeError` results, never strings.
- A slot takes jobs only after `hello` and a `self_check` with every item ok, on the first connection and after every reconnect.
- A hostile reply ends the connection cleanly without harming the supervisor process.

## Scope

### Included
- Client side of the shared codec and parser; events (`Alive`, `Progress`) delivered to the caller.
- Reconnect with backoff (non-decreasing intervals) after end of stream or a refused connection; `worker_decoder_restarts_total` incremented per reconnect after a runner exit.
- Gate per slot: `hello` (protocol 1), then `self_check` with the slot's forbidden targets; any failed item -> the slot takes no jobs, `worker_selfcheck_ok 0`, `/readyz` 503 through a registered readiness check, and the status report says `decoder_ok = false`.
- Cached `Capabilities` per slot (build timestamp, NVENC fields) for the policy step and the status report.
- A protocol violation by the decoder returned as a typed violation result and the connection dropped.

### Excluded
- Slot health, `fail(Retry)` and the silence timeout after a violation (E-20/c07); the job flow (E-20/c06).

## Acceptance Criteria

**AC-1: Start-up gate (IT-09-1, AC-25)**
Given a decoder answering `hello` and a `self_check` with every item ok, and in a second run one item (`no_network`) `ok = false`
When the slot starts
Then the first slot becomes ready and `worker_selfcheck_ok` is 1; the second takes no jobs, `worker_selfcheck_ok` is 0, `/readyz` answers 503, and the next status report says `decoder_ok = false`

**AC-2: Hostile replies (IT-09-13)**
Given a decoder that sends a frame of 64 KiB + 1, a reply with an unknown field, `width` 0, `width` 4294967295, a negative `duration_ms`, an unknown media kind, invalid UTF-8
When each arrives
Then each yields a typed violation and a dropped connection, the supervisor process stays up, and its memory grows by less than 10 MiB over the cases

**AC-3: Reconnect order (IT-09-15)**
Given a runner that exits while idle, and a decoder that refuses connections for 3 s
When the client reconnects
Then attempts back off with non-decreasing intervals, the first requests on the new connection are `hello` then `self_check`, the slot claims nothing before the gate passes, and `worker_decoder_restarts_total` increases by 1

**AC-4: Typed errors only**
Given decoder replies `Unsupported`, `Corrupt`, `LimitExceeded{wall}`, `Stalled`, `EncoderFailed{exit_code: 1}`, `OutputInvalid`, `Busy`
When each is received
Then the caller gets the matching typed variant with its fields, and no result carries decoder text

## Non-Functional Requirements

**Reliability**
- The supervisor process never restarts because of decoder input (IT-09-13 "container start time unchanged").

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Hostile reply cases against a scripted socket peer | Typed violation; bounded memory |
| AC-3 | Backoff sequence and gate ordering | Non-decreasing; hello, self_check, then ready |
| AC-4 | Error variant mapping | One typed variant each |

## Blackbox Tests

Run in harness H2 with the fake decoder (FD) and the real decoder (RD) (E-20/c11).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | FD all ok; FD one item false | IT-09-1 | Claims only when all ok | AC-25, NFT-SEC-29 |
| AC-2 | FD hostile frames | IT-09-13 | Violation; process stays up | AC-25 |
| AC-3 | RD runner killed; FD refusing | IT-09-15 | Backoff; hello, self_check, claim | R34 |

## Constraints

- The supervisor has no Docker socket; it learns of a restart only through the socket (09 §3).

## Risks & Mitigation

**Risk 1: Reconnect storm**
- *Risk*: a crash-looping decoder makes the client spin.
- *Mitigation*: non-decreasing backoff; the slot stays not ready while the gate fails.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The `forbidden_targets` list the supervisor sends has no config key; tests use `api:8080, api:8081, db:5432, 1.1.1.1:53, example.com:443` (IT-10-2) | E-20 owner, helper 01 | open | Low |
| On self-check failure `containerization.md` says the worker "refuses to start"; 09 §3 and IT-09-1 say it stays up, takes no jobs and answers `/readyz` 503. This task follows 09 | Deployment owner | open | Low |
| The supervisor's reaction to `ProtocolMismatch` from `hello` is not stated (treated here like a failed gate: slot not ready) | E-20 owner | open | Low |
