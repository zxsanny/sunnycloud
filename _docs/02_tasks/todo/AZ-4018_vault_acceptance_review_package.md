# Vault acceptance AT-19-1 and the external-review evidence package AT-19-2 [v1.2]

**Task**: AZ-4018_vault_acceptance_review_package
**Name**: Vault acceptance and review package
**Description**: Run the crate's user path (create, lock, unlock by passphrase and by recovery key, decrypt 100 MiB), and assemble the evidence package and finding log for the external review that gates the v1.2 release, including the residual-risk statement for the product text
**Complexity**: 2 points
**Dependencies**: AZ-4004_vault_review_plan_spike, AZ-4015_vault_vectors_ci_gate, AZ-4016_vault_fuzz_targets, AZ-4017_vault_perf_security_tests
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4018
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c15

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_format.md`, `vault_core_api.md` — handed to the reviewer.
- Record of E-29/c01 (review plan); `components/19_vault_core/tests.md` AT-19-1, AT-19-2; `tests/security-tests.md` NFT-SEC-43.

## Problem

v1.2 ships the vault only after an external review of the construction has no unresolved findings (AC-62). The reviewer needs the format document, the vectors, the fuzz corpus and results, and the open points of 19 §3 and §11 in one package. Residual risks (offline guessing against a stolen header, metadata exposure, a server-controlled tree that can move, hide or roll back files) must be stated in the product text (NFT-SEC-43). The review itself is an owner action and cannot be replaced by internal tests.

## Outcome

- AT-19-1 passes natively on DS-18's 100 MiB file.
- An evidence package: `vault_format.md`, `vault_core_api.md`, the vector file and oracle, fuzz corpus and results, PT and ST results, the open points of 19 §3 and §11, and this epic's flagged concerns.
- A finding log with one row per reviewer finding (status open or resolved, the commit or decision that resolves it).
- The residual-risk statement written for the product text and handed to the vault page (E-26/n01).
- AT-19-2 recorded as passed only when the report is received and the log has 0 unresolved findings; until then it is `NOT-RUN(EXT)`.

## Scope

### Included
- AT-19-1; the package; the finding log; the residual-risk text.

### Excluded
- The review itself (owner, external reviewer); fixes for findings (new tasks in this epic, named in the log).

## Acceptance Criteria

**AC-1: User path works (AT-19-1, AC-13, AC-14)**
Given a native build and the 100 MiB DS-18 file
When the test generates a passphrase, creates a vault keeping the recovery key, encrypts the file, locks, unlocks with the passphrase and decrypts, locks, unlocks with the recovery key and decrypts
Then the recovery key is returned once, the ciphertext differs from the plaintext, and both decrypted hashes equal the original

**AC-2: Package complete (AT-19-2 step 1)**
Given the review plan of E-29/c01
When the package is assembled
Then it contains every item listed in Outcome, each with its version or date, and the reviewer's receipt is recorded

**AC-3: Release gate honest (AT-19-2 step 2, AC-62)**
Given the finding log
When the v1.2 release checklist reads it
Then AT-19-2 is passed only with a received report and 0 unresolved findings; otherwise it is `NOT-RUN(EXT)` or failed, and no internal result is recorded in its place

**AC-4: Residual-risk text (NFT-SEC-43)**
Given 19 §3 and §7
When the statement is written
Then it states offline guessing against a stolen header, metadata exposure (sizes, counts, tree shape, timing) and the server-controlled tree, and is handed to E-26/n01

## Non-Functional Requirements

**Reliability**
- The package is regenerated when the format version changes.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None beyond AT-19-1 | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Native build, DS-18 | AT-19-1 steps | Hashes equal; recovery key once | AC-13, AC-14, FT-P-44 |
| AC-3 | Finding log | Release checklist | Pass only with report and 0 unresolved | AC-62, NFT-SEC-42 step 6 |

## Constraints

- Manual gate (`EXT`); nothing in this epic ships before it passes (ADR-016, FT-P-73).

## Risks & Mitigation

**Risk 1: Review findings late in the v1.2 schedule**
- *Risk*: findings change the format after clients are built.
- *Mitigation*: `format_version`; the fallback rule of E-29/c01; R15 schedule slack.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The external review is an owner item with no agreed reviewer, scope or budget yet (E-29/c01); v1.2 cannot be released without it | Owner | open | High |
