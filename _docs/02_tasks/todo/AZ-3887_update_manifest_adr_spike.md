# Update manifest layout and offline signing tool (spike, ADR-017)

**Task**: AZ-3887_update_manifest_adr_spike
**Name**: Update manifest ADR spike
**Description**: Confirm the update manifest layout proposed in ADR-017 with the owner, decide the offline signing tool (form, location, inputs, outputs, key handling) and record both in ADR-017 so the updater and the update fixtures can be built
**Complexity**: 3 points
**Dependencies**: None
**Component**: 14 desktop_daemon
**Tracker**: AZ-3887
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c04

## Problem

The owner decided (S4-3) that the update manifest is signed offline, that the app pins two Ed25519 public keys (active, recovery) and that the manifest carries `version`, per-OS SHA-256, a monotonic `serial` and an `expires` time, with the exact layout left to an ADR. ADR-017 Notes now hold a proposed layout (`format`, `version`, `serial`, `expires`, per-platform `asset`, `sha256`, `size`, and a detached `.sig` with a `key` hint), but it is not confirmed, and the signing tool is still open. The updater (E-24/c18) and the `mock-update-server` fixtures (E-31/c14) need one fixed layout.

## Outcome

- ADR-017 records the confirmed layout (field names, per-OS key names, signature file, serial storage in `meta`) and the owner's confirmation date; 14 §15 risk 4 says resolved.
- ADR-017 records the signing tool decision: what it is (a small program or script in the repository), where it lives, its inputs (manifest file, key file held offline), its output (the detached signature), how it refuses to run with a key read from an environment variable or CI secret, and which task builds it (proposed: E-24/c18, which shares the manifest type).
- A throwaway prototype signs and verifies one manifest with a test key pair, proving the bytes-as-signed rule (no canonicalisation step).
- Unblocks E-24/c18 and E-31/c14.

## Scope

### Included
- Owner review of the ADR-017 proposed layout; changes the owner asks for.
- The signing tool decision and a throwaway prototype.
- The release runbook step (who signs, when, with which key) as text in ADR-017.

### Excluded
- The production updater and the production tool (E-24/c18); the GitHub release workflow (CI epic); Windows code signing (R19, D-1).

## Acceptance Criteria

**AC-1: Layout confirmed**
Given the layout in ADR-017 Notes
When the owner reviews it
Then ADR-017 states the confirmed layout and the confirmation date, every field of the updater checks (signature, `expires`, `serial`, `version`, per-OS digest, size) has a named place, and 14 §15 risk 4 is marked resolved

**AC-2: Signing tool decided**
Given the open point in ADR-017
When the spike ends
Then ADR-017 names the tool's form, location, inputs and output, states that it reads the private key only from a file the operator provides at signing time, and names the task that builds it

**AC-3: Bytes-as-signed proven**
Given the prototype and a test key pair
When it signs a manifest and a verifier checks the exact bytes
Then verification passes, and changing one byte of the manifest or of the signature makes it fail

**AC-4: Fixture inputs named**
Given the eight bad and two good builds of DS-31
When the layout is fixed
Then the ADR or E-31/c14's fixture notes list how each of the ten cases is produced with the layout and test keys

## Non-Functional Requirements

**Reliability**
- No real updater private key is created, copied or used during the spike; only test keys.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Prototype sign and verify | Pass on exact bytes; fail on any changed byte |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| — | None: document outcome | — | — | — |

## Constraints

- The workflow never holds an updater key (ADR-017); the decision must keep that true.

## Risks & Mitigation

**Risk 1: Owner unavailable**
- *Risk*: the layout stays unconfirmed and the updater waits.
- *Mitigation*: the proposed layout is already the minimum for S4-3; the updater may start against it with the risk recorded.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The offline signing tool has no build row in any epic; proposed owner E-24/c18 | E-24 | open | Medium |
