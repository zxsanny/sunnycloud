# Block-rewrite rule and external encryption review scope (spike)

**Task**: AZ-3721_blob_rewrite_rule_spike
**Name**: Block-rewrite rule spike
**Description**: Get the owner's decision on what the blobstore does when one block is written twice, unblock ST-04-8, and write the scope of the external review of the blob encryption
**Complexity**: 3 points
**Dependencies**: None
**Component**: 04 blobstore
**Tracker**: AZ-3721
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c01

## Problem

The blobstore seals each 64 KiB chunk with the raw cipher and a hand-built STREAM nonce, because the sequential encryptor cannot jump to chunk `i` (R08). If an unacknowledged block is written again with different bytes, two plaintexts are sealed under one key and nonce. The component description leaves the rule to the caller ("security-review item"), so ST-04-8 is BLOCKED (`tests/spec-gaps.md`, owner decision). The external review of the construction is a v1.0 release gate (`canonical-names.md`, open owner items), but nobody has written what the reviewer gets.

## Outcome

- A written owner decision between the two options the epic names: reject any second write of a block index, or accept only a byte-identical second write. It states the typed error for a refused write and how the blobstore tells "same bytes" apart.
- The decision is recorded in the blobstore component description (BlobWriter `write_block` row and Risks table) and ST-04-8 in its `tests.md` has steps and pass/fail criteria; `tests/spec-gaps.md` moves ST-04-8 to resolved. This unblocks ST-04-8 in E-10/c14.
- An "External review scope" section in the blobstore component description: items to review, the evidence package, who picks the reviewer, and the deadline (before v1.0).

## Scope

### Included
- Owner decision meeting input: the two options, their cost (an identical-bytes check needs a read and compare of the stored chunk; reject needs the caller's resend rules to never rewrite), and the effect on 03's caller-side rule (ST-03-2).
- Updates to the blobstore component description, its `tests.md` (ST-04-8) and `tests/spec-gaps.md`.
- A note to the E-14 owner when 03's resend behaviour must change (no change to 03 here).
- The external review scope text.

### Excluded
- Implementing the rule (E-10/c06) or the test (E-10/c14).
- Running the external review itself (owner action) and re-encrypting data keys after a master-key leak (R22, accepted).

## Acceptance Criteria

**AC-1: Rule recorded**
Given the two options named in the epic
When the owner decides
Then the blobstore description states the rule for a second `write_block` of the same index, the typed error if it is refused, and how equality is checked, with no "pending" text left in that row

**AC-2: ST-04-8 unblocked**
Given the decision
When ST-04-8 in the blobstore `tests.md` is rewritten
Then it has an attack vector, procedure, pass criteria and fail criteria that a test can assert, and `tests/spec-gaps.md` lists ST-04-8 as resolved with the date

**AC-3: Caller rule checked**
Given 03's caller-side rule in ST-03-2 (hash verified before `write_block`)
When it is compared with the decision
Then either it is confirmed sufficient, or a written note to the E-14 owner names the change 03 needs

**AC-4: Review scope written**
Given the hand-built nonce, the data-key wrap with AAD = blob id, the key check, rewrap and the rewrite rule
When the review scope is written
Then the description holds a section listing each item to review, the evidence (known-answer vectors of IT-04-2 and the nonce construction), the owner who selects the reviewer and the deadline before the v1.0 release

## Non-Functional Requirements

**Reliability**
- The chosen rule never lets two different plaintexts at one block index both read back as valid (ST-04-8 fail criterion).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| - | none (decision task) | - |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Updated description | Document review | Rule, error and equality check stated; no pending text | |
| AC-2 | Updated `tests.md`, `spec-gaps.md` | Document review of ST-04-8 | Procedure and pass/fail criteria present; gap resolved | ST-04-8 |
| AC-3 | ST-03-2 text | Comparison | Confirmed or note to E-14 owner | |
| AC-4 | Description | Document review | Review scope section complete | |

## Constraints

- Only the two options of the epic are put to the owner; a third (allow rewrites) is not offered because ST-04-8's fail criterion forbids it.
- Behavioural decision only: the spec text says what happens, not how it is coded.

## Risks & Mitigation

**Risk 1: Decision delays E-10/c06**
- *Risk*: `write_block` is built before the rule exists.
- *Mitigation*: the blob-writer contract states the rule is pending and callers must not rely on a second write; the rule is added as a minor contract version.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Block-rewrite rule (ST-04-8) is an owner decision | Owner | open | Medium |
| Who reviews the blob encryption and when (v1.0 release gate, `canonical-names.md`) | Owner | open | Medium |
