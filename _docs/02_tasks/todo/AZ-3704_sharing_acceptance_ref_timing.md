# Sharing acceptance AT-06-1..3 and the timing measurement on REF-SRV

**Task**: AZ-3704_sharing_acceptance_ref_timing
**Name**: Sharing acceptance and reference timing
**Description**: Run the three owner-visible link scenarios at the port boundary, and run the sharing performance scenarios on the reference server to record whether missing, expired and revoked links stay within 1 ms of each other
**Complexity**: 2 points
**Dependencies**: AZ-3699_share_password_access_grant, AZ-3700_share_authorize_content, AZ-3701_share_abuse_limits, AZ-3703_sharing_security_perf_suites; notes: E-31/c01 (test epic E-31, decomposed at greenfield Step 9); E-31/c20 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 06 sharing
**Tracker**: AZ-3704
**Epic**: AZ-3619 (E-16)
**Ref**: E-16/c10

## Problem

AC-12's timing promise (median within 1 ms) and the open and authorize latency bounds only mean something on the reference server (R03). The owner also needs three acceptance scenarios that read like their use of the feature: share a photo, protect a link with a password, and keep a link across edits and moves until the file is deleted.

## Outcome

- AT-06-1, AT-06-2 and AT-06-3 pass at the port boundary in CI.
- PT-06-1, PT-06-2 and PT-06-3 run on REF-SRV with results recorded in the performance report; if REF-SRV is not available the run is recorded as `NOT-RUN(HW-REF)` in `_docs/_process_leftovers/`, never as PASS.

## Scope

### Included
- The three acceptance tests as 06 `tests.md` specifies them (DS-03 photo for AT-06-1).
- Execution of the E-16/c09 performance scenarios on REF-SRV and the recorded numbers, including the hardware description the report requires.

### Excluded
- Writing the performance scenarios (E-16/c09); the system scenarios FT-P-36..38 and NFT-SEC-17 (E-31).

## Acceptance Criteria

**AC-1: Share a photo and open it (AT-06-1, AC-12, AC-47)**
Given an account with a JPEG from DS-03
When the owner creates a link with the default expiry, a visitor opens it, content is authorized with `starts_download = true`, and the owner revokes it and the visitor opens it again
Then the token is shown once with `expires_at` 7 days ahead; the visitor sees `Available` with the name, size and `image/jpeg`; authorization returns the current revision and one `share_accessed` event; after revoke the answer is `NotFound` within the cache window

**AC-2: Password-protected link (AT-06-2, AC-20)**
Given a link with a password
When a visitor opens it, sends 10 wrong passwords and an 11th, waits out the window, sends the right password and requests content with the grant
Then the open gives `PasswordRequired`; the wrong passwords give `WrongPassword` 10 times and then `RateLimited{retry_after}`; after the window the right password yields a grant and authorization returns `ShareContent`

**AC-3: A link survives edits and moves and dies with delete (AT-06-3, AC-34, AC-77)**
Given a link on `notes.txt`
When the owner saves in place, renames and moves the file, then deletes it
Then the same link returns the new revision after the edit, still works after rename and move, and answers `NotFound` within 5 s of the delete

**AC-4: Timing measured on the reference server (PT-06-1..3, AC-12)**
Given REF-SRV as `tests/environment.md` describes it
When PT-06-1, PT-06-2 and PT-06-3 run there
Then the report records `open` p95 (target 10 ms), `authorize_content` p95 (target 15 ms), the largest median difference between missing cases (target 0.3 ms, fail above 1 ms), `open` p95 during the password flood (target 50 ms) and the peak permits in use, each marked pass or fail against its threshold; without REF-SRV each is `NOT-RUN(HW-REF)` with a leftover record

## Non-Functional Requirements

**Performance**
- Link opens end to end within 2 s (AC-12) is measured at system level (FT-P-36); here the port-level numbers of AC-4 apply.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | Acceptance and measurement only | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | JPEG from DS-03 | AT-06-1 | Steps 1-4 as stated | FT-P-36, FT-P-39 |
| AC-2 | Password link | AT-06-2 | `PasswordRequired`, 10 x `WrongPassword`, `RateLimited`, then grant | FT-N-10, NFT-SEC-06 |
| AC-3 | Link on `notes.txt` | AT-06-3 | New revision; survives moves; `NotFound` within 5 s | FT-P-57, FT-N-44 |
| AC-4 | REF-SRV | PT-06-1..3 on REF-SRV | Numbers recorded against thresholds, or `NOT-RUN(HW-REF)` | NFT-SEC-17 |

## Constraints

- Higher-tier timing runs only on the designated host; a workstation run is not reported as a result (`tests/environment.md`).

## Risks & Mitigation

**Risk 1: Reference hardware not provisioned (R03)**
- *Risk*: AC-12's timing stays unproven at release.
- *Mitigation*: `NOT-RUN(HW-REF)` with a leftover record makes the gap visible to the release gate (E-31/c47).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| REF-SRV model and availability are owner items (performance gaps G1, G11; E-31/c01) | Product owner | open | Medium |
| AT-06-2 step 3 depends on the G-06-2 decision only if the right password is sent before the window ends; this scenario waits for the window | E-16 owner | resolved | Low |
