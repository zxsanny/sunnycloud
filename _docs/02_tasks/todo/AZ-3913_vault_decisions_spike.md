# Vault decisions: membership mechanism, vaults per account, header cap (spike)

**Task**: AZ-3913_vault_decisions_spike
**Name**: Vault decisions spike
**Description**: Record how the server knows a node lies under a vault root, whether an account may hold one vault or many, and the default of `max_vault_header_bytes`, so the vault service, its migration and the commit-path wiring can be built without guessing
**Complexity**: 3 points
**Dependencies**: None
**Component**: 20 vault_service [v1.2]
**Tracker**: AZ-3913
**Epic**: AZ-3648 (E-30)
**Ref**: E-30/c01

## Problem

Three open items in `tests/spec-gaps.md` block the vault service. First, vault membership: 02 must set the `vault` flag when it publishes a revision, 03 must apply `max_vault_file_bytes` at upload start, and 06 must refuse a share of a vault file. All three must learn that a node is under a vault root without calling 20 (ADR-015). The spec does not say how, nor what happens for nodes deeper than one level, for moves into, out of and inside a vault, or for a plaintext file that already has a share link and is moved into a vault. Second, one or many vaults per account is undecided (data model A6), so `VaultExists` is only "if decided" and the second-vault case of IT-20-7 is BLOCKED. Third, `max_vault_header_bytes` has no default, so the max and max + 1 cases of IT-20-1 are BLOCKED. A6 also leaves open whether vault names are ciphertext, while NFT-SEC-43 step 5 already expects no plaintext name on the server.

## Outcome

- One written answer per item, recorded in `components/20_vault_service/description.md` (§3, §6, §10) and in the matching rows of `tests/spec-gaps.md` with the date.
- The answers unblock E-30/c02 (header cap default), E-30/c03 (`VaultExists`), E-30/c07 (membership wiring) and the BLOCKED parts of IT-20-1 and IT-20-7.
- Every data-model or port change the answers need is named as a follow-up task in its owning epic.

## Scope

### Included
- Membership: the mechanism that answers "is this node under a vault root" from the node or its ancestry, and where 02, 03 and 06 read it (publish flag, upload-start size check, share create).
- Membership edge cases: nodes deeper than one level; moving a file into a vault, out of a vault and between folders of the same vault; a plaintext file with an active share link moved into a vault.
- Vaults per account: one or many, and whether `VaultExists` stays in the port.
- The default of `max_vault_header_bytes`, computed from the header layout of 19 with a stated margin.
- The A6 name question: the owner's answer, or the item kept open with NFT-SEC-43 step 5 aligned to it.

### Excluded
- Building any of it (E-30/c02, c03, c07).
- Vault cryptography and the header format itself (E-29).
- How vault crates are kept out of v1.0 and v1.1 builds (module-layout exception 6; flagged on E-22/c03).

## Acceptance Criteria

**AC-1: Membership mechanism recorded**
Given the rule that membership is answerable without calling 20
When the spike ends
Then 20 §2 and §10 name the mechanism, the point where each of 02, 03 and 06 reads it, and the rule for nodes deeper than one level, for moves into, out of and inside a vault, and for a shared plaintext file moved into a vault; the "Vault membership" row of `tests/spec-gaps.md` shows the owner's decision and the date

**AC-2: Vaults per account recorded**
Given data-model item A6
When the spike ends
Then 20 §3 states one vault or many per account; with one, `VaultExists` and its case in IT-20-7 are defined; with many, `VaultExists` is removed from the port proposal; the IT-20-7 second-vault case is no longer BLOCKED

**AC-3: Header cap default recorded**
Given the header fields of 19 (format version, KDF bounds, two wrapped vault keys, key-commitment value)
When the spike ends
Then 20 §6 gives a default for `max_vault_header_bytes` with its computation (largest valid header plus a stated margin); the max and max + 1 cases of IT-20-1 are READY; the "Header cap" row of `tests/spec-gaps.md` is resolved with the date

**AC-4: Follow-ups named**
Given any answer that changes a table of 02, 03 or 06, or a port in `contracts`
When the spike ends
Then each change is listed as a task in its owning epic (E-13, E-14, E-16 or E-00) with its ref, E-30/c07 lists those tasks as dependencies, and no work moves across epics

**AC-5: Vault names**
Given NFT-SEC-43 step 5 (0 plaintext name markers on the server)
When the spike ends
Then the A6 name question is either decided by the owner and written in 20 §5, or recorded as open with NFT-SEC-43 step 5 marked BLOCKED on it

## Non-Functional Requirements

**Reliability**
- The chosen mechanism adds no call from 02, 03 or 06 to 20 on the commit or share path (ADR-015).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None: document outcome | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | 20 description, spec-gaps | Review of the membership record | Mechanism, read points, depth, moves and shared-file rule present; owner decision dated | IT-20-4, IT-20-5 |
| AC-2 | 20 §3, IT-20-7 | Review of the vaults-per-account record | One or many stated; `VaultExists` defined or removed; IT-20-7 READY | AC-59 |
| AC-3 | 19 header layout | Review of the cap computation | Default and margin written; IT-20-1 READY | AC-61 |
| AC-4 | The answers | Review of the follow-up list | Each change has an owning epic and ref | |
| AC-5 | NFT-SEC-43 | Review of the name record | Decided, or open with step 5 BLOCKED | AC-13 |

## Constraints

- Decisions go into the existing documents; no new document.
- Membership, vaults per account and vault names are owner decisions; the header cap is a decompose-level decision (`tests/spec-gaps.md`).

## Risks & Mitigation

**Risk 1: The header format changes after the external review (R23)**
- *Risk*: a cap computed from today's layout becomes too small.
- *Mitigation*: the cap carries a margin and stays a config key; the record names E-29/c01 as the trigger to revisit it.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Membership, vaults per account and vault names need owner decisions; the spike can only propose | Product owner | open | Medium |
| `system-flows.md` F17 description ("sharing is by link with the key in the URL fragment") and NFT-SEC-42 step 5 contradict the owner decision that vault files cannot be link-shared in v1.2 (`canonical-names.md`); doc fix needed | Doc owner / E-31 | open | Low |
