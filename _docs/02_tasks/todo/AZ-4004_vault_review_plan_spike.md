# Spike: external review plan, format freeze and libsodium fallback decision [v1.2]

**Task**: AZ-4004_vault_review_plan_spike
**Name**: Vault review plan and format freeze
**Description**: Agree with the owner the external review of the vault construction (scope, reviewer, budget, schedule), freeze format version 1 for that review, and decide when the libsodium `secretstream` fallback replaces `aead-stream`
**Complexity**: 3 points
**Dependencies**: None
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4004
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c01

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_format.md` — the byte-layout proposal this spike freezes.
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — the operations under review.
- `components/19_vault_core/description.md` §3, §11; ADR-007; R23, R35; `acceptance_criteria.md` AC-62.

## Problem

The vault ships only after an external review of the construction passes (AC-62, RESTRICT-03 "Encryption and vault"). 19's format is a proposal for that review, not a frozen format, and `aead-stream` is unaudited (2026 first release); `chacha20poly1305`'s 2020 audit may not cover XChaCha. The parent folder is deliberately not bound (R35, Step 5 owner decision), which the reviewer must accept. 19 §4 says implementation starts only after the review plan is agreed and the format fixed. Scope, reviewer and budget are owner decisions; they cannot be replaced by an internal check.

## Outcome

- A written review plan, signed off by the owner, recorded in `components/19_vault_core/description.md` §11 risk 1: reviewer (or shortlist), scope, budget, schedule against v1.2, what is handed over, how findings are tracked.
- `vault_format.md` frozen for format version 1 (status `frozen`, field order, byte order and lengths fixed) or the remaining open points listed with an owner and date.
- The fallback rule: which review finding switches the chunk stream to libsodium `secretstream`, and its cost (C toolchain for WASM, Android, iOS).
- Unblocks E-29/c03, c04, c06; AT-19-2 (E-29/c15) uses the plan.

## Scope

### Included
- Review scope list: header construction and bounds, key commitment, recovery-key wrap, STREAM nonce handling, AAD without the parent (R35), zeroization limits in WASM, offline guessing residual risk, name encryption question (data model A6), the `chacha20poly1305` audit coverage of XChaCha.
- The freeze of `vault_format.md` open points 1 to 3 where the owner can decide now.
- The fallback decision rule.

### Excluded
- The review itself and its report (owner action; evidence package E-29/c15).
- Any implementation.

## Acceptance Criteria

**AC-1: Review plan agreed**
Given the scope list above
When the spike ends
Then 19 §11 records the reviewer (or a shortlist with a decision date), budget, schedule, handover contents and finding tracking, with the owner's sign-off and date

**AC-2: Format frozen or open points owned**
Given `vault_format.md` 1.0.0 (draft)
When the spike ends
Then either the contract status is `frozen` with field order, byte order and header length fixed, or each remaining open point has an owner and a date, and no implementation task treats an open point as decided

**AC-3: Fallback rule**
Given R23
When the spike ends
Then the record states which review outcome triggers the libsodium fallback, that it changes `format_version`, and its toolchain cost per target

**AC-4: No shortcut around the review**
Given the release gate AC-62
When the plan is recorded
Then it states that no internal test, fuzz result or vector gate replaces the external review, and that v1.2 is not tagged while the review has unresolved findings

## Non-Functional Requirements

**Reliability**
- The plan is versioned with the format; a format change after the freeze re-opens AC-2.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (decision record) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Owner meeting | Record review | Plan with sign-off | AC-62 |
| AC-2 | `vault_format.md` | Contract status | Frozen, or open points owned | AC-61 |

## Constraints

- The reviewer, scope and budget are owner decisions (E-29 external dependencies).

## Risks & Mitigation

**Risk 1: The review changes the format late**
- *Risk*: findings arrive after implementation.
- *Mitigation*: `format_version` from day one; the fallback rule of AC-3; v1.0 and v1.1 are unaffected.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| External review scope, reviewer and budget are not yet agreed; nothing in E-29 ships before the review passes (AC-62) | Owner | open | High |
| `aead-stream` is unaudited and the `chacha20poly1305` audit may not cover XChaCha (R23) | Owner / reviewer | open | Medium |
| Name encryption (data model A6) is undecided; `seal_name`/`open_name` exist only if the review says names are ciphertext | Owner | open | Medium |
