# Spike S12: verified download up to 2 GiB in Firefox and Safari with a bounded heap

**Task**: AZ-3931_s12_verified_download_spike
**Name**: S12 Firefox and Safari verified-download spike
**Description**: Measure candidate mechanisms that download and hash-verify files up to 2 GiB in Firefox and Safari without holding the file in the JavaScript heap, and record the chosen mechanism or the gate-S12 decision needed
**Complexity**: 3 points
**Dependencies**: AZ-3930_vite_wasm_spike, AZ-3652_initial_structure
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3931
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c02

## Problem

AC-7 requires Firefox and Safari to verify the hash in the browser for files up to 2 GiB, and AC-51 caps the browser heap at 200 MB. The description names no mechanism ("fixed by spike S12": streamed fetch with incremental hashing and a disk-backed save, no whole-file `Blob` in the heap). IT-16-8 is BLOCKED on it (`components/16_web_app/tests.md`, `tests/spec-gaps.md` row IT-16-8), and so is the Firefox/Safari path of E-26/c12 (risk R30, release gate S12).

## Outcome

- At least two candidate mechanisms measured in Firefox and Safari at 1 GiB and 2 GiB: peak heap, hash result and duration.
- A decision recorded in `components/16_web_app/description.md` §5 Downloads step 2, in IT-16-8 of `components/16_web_app/tests.md` (BLOCKED clause replaced by input and expected heap) and in the IT-16-8 row of `tests/spec-gaps.md`.
- If no candidate passes in an engine, the record says so and names the owner decision gate S12 needs; E-26/c12 then builds the labelled unverified path for that engine.

## Scope

### Included
- Candidate mechanisms that keep the file out of the JavaScript heap while hashing it block by block with the WASM hasher from the E-26/c01 probe.
- Runs in Firefox (`browsers` container) and Safari (owner Mac, `safaridriver`) at 1 GiB and 2 GiB; heap from the Firefox memory report and Safari `vmmap` footprint (weak evidence, labelled).
- A corrupt-block run per candidate and a check under the CSP of 16 §5.

### Excluded
- The production download code (E-26/c12); the Chromium save-picker path (already defined); downloads above 2 GiB (native download, labelled unverified).

## Acceptance Criteria

**AC-1: Candidates measured**
Given at least two candidate mechanisms and server files of 1 GiB and 2 GiB
When each file is downloaded with each candidate in Firefox and in Safari
Then the record holds, per candidate, engine and size, the peak heap, whether the saved file's hash equals the server's, and the duration; Safari values are labelled weak evidence, and a run without the Mac is recorded `NOT-RUN(HW-DEVICE)`, never as a pass

**AC-2: Pass rule applied**
Given the measurements
When a candidate keeps the peak heap at or below 200 MB (target 160 MB) and saves a 2 GiB file whose hash equals the server's in both engines
Then the record names it as the chosen mechanism; otherwise it names the failing engine and the owner decision gate S12 needs (lower verified limit, or the labelled unverified path)

**AC-3: Corruption is never saved as good**
Given the chosen candidate and a server copy with one corrupt block
When the file is downloaded
Then the mismatch is detected before the page could report the file as saved, and the record states per engine what is left on disk (partial file removed, or left under a non-final name)

**AC-4: Works under the CSP**
Given the chosen candidate
When it runs under exactly the CSP of 16 §5
Then 0 CSP violations are reported, or the record names the extra directive needed and hands it to the deployment owner of the CSP header

**AC-5: Spec gap closed**
Given the decision
When the spike closes
Then the 16 description §5, IT-16-8 in the 16 test spec and the IT-16-8 row in `tests/spec-gaps.md` are updated with the date, and E-26/c12 can write its Firefox and Safari criteria from the record

## Non-Functional Requirements

**Performance**
- Heap target 160 MB, failure above 200 MB (AC-51, PT-16-2).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Incremental hash of a stream with one flipped block against the expected file hash | Mismatch reported at the end of the stream, never a match |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | 1 GiB and 2 GiB files on the real server; Firefox; Safari on H-MAC | Download with each candidate, heap sampled | Values recorded; pass rule applied | AC-7, AC-51, IT-16-8 |
| AC-3 | One corrupt block (seeded on the volume) | Download with the chosen candidate | Mismatch detected; disk state recorded | NFT-SEC-21 |
| AC-4 | 16 §5 CSP | Chosen candidate | 0 violations or the needed directive recorded | IT-16-9 |

## Constraints

- No whole-file `Blob` or `ArrayBuffer` in the heap (16 §5 Downloads step 2).
- The hash must come from the WASM hasher, never a JavaScript implementation.

## Risks & Mitigation

**Risk 1: No candidate passes in Safari**
- *Risk*: Safari may offer no disk-backed streaming save.
- *Mitigation*: AC-2 records the failing engine and the owner decision; the labelled unverified path stays correct (AC-7 wording "not verified in the browser").

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| If no candidate passes in Safari or Firefox, AC-7's "verify up to 2 GiB" cannot hold for that engine; gate S12 needs an owner decision | Owner (gate S12) | open | Medium |
| Safari runs need the owner Mac (H-MAC) | Owner (hardware) | open | Low |
