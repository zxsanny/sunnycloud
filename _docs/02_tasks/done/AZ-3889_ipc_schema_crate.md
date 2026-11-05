# ipc_schema crate: DaemonControl types with no token types

**Task**: AZ-3889_ipc_schema_crate
**Name**: IPC schema crate
**Description**: The shared schema between the daemon and its local clients: the `DaemonControl` trait, every method's parameter, result and error type, the JSON-RPC 2.0 mapping and line framing, with a schema walk proving that no type can carry a token
**Complexity**: 3 points
**Dependencies**: AZ-3652_initial_structure
**Component**: 14 desktop_daemon (crate `ipc_schema`)
**Tracker**: AZ-3889
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c05

## Problem

The settings window (15) talks to the daemon only through JSON-RPC and may import nothing of 14 except the `ipc_schema` crate. The tray calls the same surface in-process. Both sides need one definition of method names, types and errors, or they drift. AC-48 also requires that no method can return a token, which must be checkable by a test that walks the schema rather than by review.

## Outcome

- The `ipc_schema` crate exposes the `DaemonControl` trait and every type and error of `contracts/14_desktop_daemon/ipc_schema.md`, plus the JSON-RPC request, response, error and notification envelopes and the newline framing rule.
- E-25's stub daemon and `DaemonClient` and E-24's IPC server, tray and control implementation build against it.
- A schema-walk test fails the build if any reachable type is or contains a token, secret or credential.

## Scope

### Included
- The trait, parameter, result and error types, and `StatusEvent`, as listed in the contract.
- JSON-RPC 2.0 envelopes, the error mapping (`ControlError` to code -32000 with `data.kind`; -32700, -32601, -32602 cases) and one-object-per-line framing helpers usable by both sides.
- Round-trip tests for every type; the schema-walk test (ST-14-2 static half).

### Excluded
- The socket or pipe endpoint and its ownership rules (E-24/c16); the trait's implementation (E-24/c19); the client (E-25).

## Acceptance Criteria

**AC-1: Every method and type exists once**
Given the method table of the contract
When the crate is built
Then each of the 18 method names, each parameter, result and error type, and `StatusEvent` exist with the contract's spellings, and the crate depends on no `sync-core` crate and no daemon module (crate-graph check of AZ-3652)

**AC-2: No token type reachable (AC-48)**
Given every type reachable from the method results and `StatusEvent`
When the schema-walk test runs
Then it finds 0 fields typed `Token`, `Secret` or `Credential` and 0 fields named `token`, `secret` or `password`, and a deliberately added field named `token` in a test copy makes it fail

**AC-3: Exactly one pair target**
Given an `add_pair` request with both a server node id and a server path, and one with neither
When it is decoded
Then both fail with JSON-RPC -32602 Invalid params, and a request with exactly one target decodes

**AC-4: Error mapping**
Given each `ControlError` variant with its fields (for example `RulesInvalid{line: 3}`, `SettingInvalid{key}`, `UpgradeRequired{message}`)
When it is encoded as a JSON-RPC error
Then it carries code -32000, the variant name as `message` and `{"kind": ..., fields}` as `data`, and decodes back to the same variant; an unknown method maps to -32601 and an unparsable line to -32700

**AC-5: Framing and text fidelity**
Given a rules text with a trailing newline and non-ASCII characters inside a request
When it is framed as one line and decoded
Then the decoded string equals the original byte for byte, and two requests on one stream decode as two messages

**AC-6: Forward-compatible reading**
Given a `DaemonStatus` JSON with an extra unknown field
When it is decoded
Then decoding succeeds and the known fields are intact

## Non-Functional Requirements

**Compatibility**
- Builds for Linux, Windows and macOS targets; no OS-specific code.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Method-name list against the contract table | 18 names, exact spellings |
| AC-2 | Schema walk (ST-14-2 static half) | 0 forbidden fields; injected `token` field fails |
| AC-3 | `PairSpec` decoding: both, neither, one | -32602, -32602, ok |
| AC-4 | Encode and decode each `ControlError` variant; -32601; -32700 | Same variant back; codes as listed |
| AC-5 | Framing round trip with trailing newline and non-ASCII | Byte-identical; two messages |
| AC-6 | `DaemonStatus` with an unknown field | Decodes |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| — | None: library without I/O; the wire runs are E-24/c16 (IT-14-16, ST-14-2 runtime half) | — | — | — |

## Constraints

- Serde glue only (and helper error if wire codes are needed); no network, no file I/O.
- Field types left open by the specs are taken from the contract's proposals; changing one later is a contract version change.

## Risks & Mitigation

**Risk 1: 15 and 14 compiled against different versions**
- *Risk*: a field rename breaks one side silently.
- *Mitigation*: both binaries build from one workspace and one crate; the contract's versioning rules apply.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Per-file errors (FT-N-51 rejected names, the mockup's Status "Errors" list) have no field or method in the schema | E-24 with owner | open | Medium |
| Endpoint names (socket, directory, pipe) are not fixed; 15 §1 points to helper 01, which has no such rule (contract open point 1) | E-24 | open | Low |
| Field types of `account`, `update_available`, `access_denied`, `watcher_degraded`, hold counts and `LogoutOutcome` are proposals (contract open point 3) | E-24 | open | Low |
| `LoginPrompt.expires_at` has no source in 13's `DeviceAuthorization` (E-23/c13) | E-23 | open | Low |
| Bandwidth limit unit (G-AS-2) | Owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
