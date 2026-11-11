# Tree spike: Unicode name form and CommitEffect contract-test plan

**Task**: AZ-3705_tree_spike_names_effect_tests
**Name**: Tree name-form and effect-test spike
**Description**: Get the owner's decision on the NFC/NFD name form (G-1), confirm the escaping-marker items do not change server name rules, and write the contract-test plan for `CommitEffect` with the E-14 and E-18 owners
**Complexity**: 2 points
**Dependencies**: None
**Component**: 02_tree
**Tracker**: AZ-3705
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c01

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_commit.md` — commit contract whose Test Cases this spike completes.
- `_docs/02_document/tests/spec-gaps.md` — rows G-1 and ST-14-5.

## Problem

Two open items block a firm name rule and a stable effect contract. G-1 (an owner item in `tests/spec-gaps.md`) asks whether `é` written as NFC and as NFD is one name or two, and which form is stored; IT-02-1 and FT-P-15 assume "one name" today. The Windows reserved-name marker (`%5F`) and the ` (2)` suffix are open owner items in `canonical-names.md`; nobody has confirmed they leave the server rules alone. Separately, `CommitEffect` is called by 02 and implemented by 03 and 08; without one agreed list of contract tests, the three sides can drift (E-13 risk 2, E-14 risk 4).

## Outcome

- A written owner decision on G-1, or a recorded "still open" with the interim rule the tree builds against.
- A written confirmation that the escaping-marker items are client-side only, or the list of server rule changes they cause.
- A contract-test plan for `CommitEffect`: each case, its test ID and the task that runs it.

## Scope

### Included
- Prepare the G-1 options (same name vs two names; stored form), with the effect of each on `name_key`, IT-02-1, FT-P-15 and AC-42, and take them to the owner.
- Check with the E-05 owner that the marker and suffix items do not change what the server accepts or how `name_key` is built.
- With the E-14 and E-18 owners, list the cases where 02's real `publish` runs with 03's real `BlobCommitEffect`/`ReceiptEffect` and 08's real media effect.
- Record results in `tests/spec-gaps.md` (G-1 row) and in the Test Cases section of the commit contract.

### Excluded
- Implementing name folding (E-05/c01) or any tree code.
- Deciding the client-side escaping itself (E-05, E-24).

## Acceptance Criteria

**AC-1: G-1 outcome recorded**
Given G-1 is open in `tests/spec-gaps.md`
When the spike ends
Then the G-1 row states either the owner's decision (one name or two, stored form) with its date, or "open" plus the interim rule the tree implements (NFC/NFD pair gives `NameConflict`, as IT-02-1 expects) and the tasks it touches (E-13/c04, E-05/c01, FT-P-15)

**AC-2: Marker items checked**
Given the open owner items on the `%5F` marker and the ` (2)` suffix
When the E-05 owner is consulted
Then a note in the G-1/ST-14-5 rows says whether server name acceptance (AC-37) or `name_key` changes; if it does, each changed rule is listed against E-13/c04

**AC-3: Contract-test plan written**
Given the commit contract
When the plan is agreed with the E-14 and E-18 owners
Then its Test Cases list at least: receipt replay under the account lock, an `Entry` failure that rolls back the entry's quota release, a `Batch` failure that rolls back everything, reaper versus commit on one session, a media job row rolled back with its entry, and no statement after the stamp; each row names its test ID (for example IT-02-10, IT-02-12, IT-03-8, IT-03-9, IT-03-19) and the task that runs it (E-13/c16, E-14/c15, E-18/c06)

**AC-4: Consumers named**
Given the plan in AC-3
When it is recorded
Then the commit contract's consumer list names E-14/c08, E-14/c09, E-14/c15 and E-18/c06

## Non-Functional Requirements

**Compatibility**
- The decision keeps `name_key_version` as the migration path if the fold changes later (R16).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (decision task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | G-1 open | Review of the G-1 row | Decision with date, or interim rule and affected tasks | FT-P-15, AC-42 |
| AC-2 | Marker items open | Review of the G-1/ST-14-5 notes | Server-rule impact stated | AC-37 |
| AC-3 | Commit contract | Review of its Test Cases | Six named cases, each with test ID and owning task | IT-02-10, IT-03-9 |
| AC-4 | Plan recorded | Contract header | Consumer list complete | |

## Constraints

- The owner decides G-1; the spike does not pick on the owner's behalf. If no answer comes, AC-1's "open" branch applies and E-13/c04 builds the interim rule.
- No code; the output is text in the two named documents.

## Risks & Mitigation

**Risk 1: Owner answer arrives after E-13/c04 starts**
- *Risk*: the name rule changes after the unique index and tests exist.
- *Mitigation*: the interim rule matches IT-02-1; a later change goes through `name_key_version` (E-05).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-1 NFC vs NFD stored form is an owner item | Owner | open | Low |
| Windows reserved-name marker and ` (2)` suffix are open owner items (`canonical-names.md`) | Owner, E-05 | open | Low |
