# Scripted fake decoder (FD) as a test-stack service

**Task**: AZ-4382_fake_decoder_service
**Name**: Fake decoder service (H2)
**Description**: A standalone scripted fake decoder that speaks the decoder protocol on the real decoder socket of the Compose test stack, records every request, and can send hostile frames and files
**Complexity**: 5 points
**Dependencies**: AZ-3852_decoder_protocol_module, AZ-3871_worker_decoder_client
**Component**: 09_worker_supervisor (test tools)
**Tracker**: AZ-4382
**Epic**: AZ-3639 (E-20)
**Ref**: split from AZ-3878 (E-20/c11, harness H2)

## Problem

H2 (`_docs/02_document/components/09_worker_supervisor/tests.md`) needs a fake decoder (FD) on the real socket for fault and protocol cases. The only one today is in-process test code (`src/worker/tests/slot_support/decoder.rs` `FakeDecoder`, `decoder_support/hostile.rs`). It cannot run beside the supervisor container.

## Outcome

- One FD binary built from the same code as the in-process `FakeDecoder` (moved into a shared test-support crate or module, not copied), with the shared `media_shared::decoder_protocol` codec.
- It reads a script file: hello and self-check answers, then per request kind an answer (probe facts, image result, transcode result, silent, alive-only, garbage frame, hang, over-time, hostile output files such as a symlink or an oversized file).
- It appends every request it receives to a JSON-lines record file that the suite reads.
- A test image and a Compose override that put FD on one decoder socket volume in place of a real decoder. The suite chooses per case which socket gets FD.

## Scope

### Included
- The binary, its script and record format (documented in the module doc), the image target, the Compose override, and a unit test of the script parser and the recorder.

### Excluded
- The IT-09 cases themselves (AZ-4384, AZ-4385); any media outcome (FD never stands in for the real decoder there).

## Acceptance Criteria

**AC-1: Scripts replay exactly**
Given a script with each answer kind
When requests arrive
Then each answer matches the script in order, and an unscripted request gets the script's default

**AC-2: Every request is recorded**
Given any session
When it ends, or FD is killed
Then the record file holds every request received, one JSON line each, flushed per line

**AC-3: On the real socket in the stack**
Given the stack with the FD override on the interactive socket
When the supervisor starts
Then it completes `hello` and `self_check` with FD, and the record shows them

**AC-4: One implementation**
Given the worker crate's in-process tests
When they run
Then they use the same FD code as the binary (no second fake)

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1, AC-2 | Script parser and recorder | Scripts replay exactly; every request recorded |

## Runtime Completeness

- FD is an allowed external stub only for fault and protocol cases (H2 rule). It must never answer a case that claims a media outcome.
