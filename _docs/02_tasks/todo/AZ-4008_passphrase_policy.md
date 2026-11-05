# Passphrase policy: `check_passphrase` and `generate_passphrase` [v1.2]

**Task**: AZ-4008_passphrase_policy
**Name**: Vault passphrase policy
**Description**: Refuse vault passphrases shorter than 12 characters or scoring below 4 on zxcvbn, and generate 6-word passphrases from a fixed 7,776-word list without bias that the check always accepts, behind the `passphrase-policy` feature
**Complexity**: 3 points
**Dependencies**: AZ-4007_vault_core_skeleton
**Component**: 19 vault_core [v1.2]
**Tracker**: AZ-4008
**Epic**: AZ-3645 (E-29)
**Ref**: E-29/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/19_vault_core/vault_core_api.md` — `check_passphrase`, `generate_passphrase`, `TooShort`, `TooWeak`, `Rng` (this task contributes them).
- `restrictions.md` RESTRICT-33; `tests/blackbox-tests.md` FT-N-37; `tests/security-tests.md` NFT-SEC-41 step 6.

## Problem

The vault header is stored on the server, so whoever holds the database can guess passphrases offline at Argon2id speed; nothing rate-limits that (19 §3, NFT-SEC-43). The only defence is a strong passphrase: at least 12 characters and zxcvbn score 4, or a generated one (RESTRICT-33). Older documents said score 2 or 3; they are stale. The dictionary and word list may be too large for WASM (19 risk 5).

## Outcome

- `check_passphrase`: `TooShort` below 12 characters (counted in characters, not bytes), `TooWeak` below score 4, Ok otherwise.
- `generate_passphrase(RandomSource)`: 6 words from the fixed 7,776-word list (about 77 bits), uniform word choice with no modulo bias; always accepted by `check_passphrase`.
- The feature `passphrase-policy` and the measured WASM size with and without it, recorded in 19 §5; the `zxcvbn-ts` fallback decision if the size is too large.

## Scope

### Included
- Both functions, the word list as a committed fixed asset, the feature flag, the WASM size measurement.
- Test IT-19-9.

### Excluded
- UI wording and where the page shows the rule (E-26/n01).

## Acceptance Criteria

**AC-1: Length rule (IT-19-9, FT-N-37)**
Given an 11-character passphrase, the empty string and 12 emoji (12 characters, 48 bytes)
When `check_passphrase` runs
Then the first two return `TooShort`, and the emoji passphrase is measured as 12 characters

**AC-2: Strength rule (IT-19-9, RESTRICT-33)**
Given a 12-character passphrase with zxcvbn score 3 and one with score 4 (checked against the reference implementation)
When `check_passphrase` runs
Then score 3 returns `TooWeak` and score 4 returns Ok

**AC-3: Generated passphrases always pass (IT-19-9)**
Given a seeded `RandomSource`
When `generate_passphrase` runs 10,000 times
Then every result is 6 words from the list, every one passes `check_passphrase`, there are no duplicates, and every word index is below 7,776

**AC-4: No bias**
Given 777,600 word draws and a source that returns the maximum value on every draw
When the distribution is tested and the maximal source is used
Then the chi-square test gives p > 0.001, and the maximal source still yields valid indexes below 7,776

**AC-5: Failing randomness**
Given a `RandomSource` that fails
When `generate_passphrase` runs
Then it returns `Rng` and no partial passphrase

**AC-6: WASM size recorded**
Given the WASM build with and without `passphrase-policy`
When both are measured
Then 19 §5 records both sizes and whether the web page uses this check or `zxcvbn-ts`; if `zxcvbn-ts`, the five IT-19-9 passphrases give the same verdicts in both

## Non-Functional Requirements

**Compatibility**
- Same verdicts on native, WASM and UniFFI for the IT-19-9 fixtures.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Length cases | `TooShort`; 12 characters for emoji |
| AC-2 | Score 3 and 4 fixtures | `TooWeak`; Ok |
| AC-3 | 10,000 seeded generations | All valid, all pass, no duplicates |
| AC-4 | Chi-square over 777,600 draws; maximal source | p > 0.001; indexes valid |
| AC-5 | Failing source | `Rng` |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | WASM build in Node | IT-19-9 fixtures | Same verdicts as native | FT-N-37, NFT-SEC-41 |
| AC-6 | Two WASM builds | Size | Recorded; fallback decided | 19 risk 5 |

## Constraints

- The rule is the one of `restrictions.md`: 12 or more characters and score 4, or generated.

## Runtime Completeness

- **Capability**: zxcvbn strength estimation.
- **Production code that must exist**: the zxcvbn check in every build that creates or rewraps a vault (or `zxcvbn-ts` on the web page if AC-6 decides so).
- **Allowed stubs**: none.
- **Unacceptable substitutes**: a length-only check, a home-made entropy formula.

## Risks & Mitigation

**Risk 1: The rule exists twice on the web**
- *Risk*: with `zxcvbn-ts`, the web and native verdicts differ.
- *Mitigation*: AC-6's verdict comparison.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The 7,776-word list is not named and its licence is not checked (the `cargo deny` allowlist covers crates, not data files) | Owner | open | Low |
| Unicode normalisation of the passphrase is not stated (`vault_core_api.md` open point 3); length counting here uses the input as given | Owner / reviewer | open | Medium |
