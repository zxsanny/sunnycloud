# AGPL-3.0 LICENSE and README layout section

**Task**: AZ-3744_license_readme
**Name**: LICENSE and README layout section
**Description**: Add the verbatim AGPL-3.0 licence text at the repository root and a README section that describes the layout and how to run the PR gate locally
**Complexity**: 1 point
**Dependencies**: AZ-3652_initial_structure
**Component**: Bootstrap (licence and docs)
**Tracker**: AZ-3744
**Epic**: AZ-3621 (E-00)
**Ref**: E-00/c12

## Problem

The project is AGPL-3.0 (ADR-011, RESTRICT-29), the `supply-chain` CI job checks that the licence file exists, and every contributor needs one place that says where code lives and which commands the PR gate runs. None of this exists yet.

## Outcome

- The repository root carries the unmodified AGPL-3.0 text.
- The README explains the folder layout of AZ-3652 and lists the local pre-push commands of `ci_cd_pipeline.md`, adapted to the `src/` workspace root.

## Scope

### Included
- The AGPL-3.0 licence text, verbatim from the FSF, at the repository root.
- A README section: top-level folders and the `src/` crate groups (from AZ-3652's Project Folder Layout), the rule that cargo commands run in `src/`, and the local PR-gate commands (`cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, the audit and deny commands, the sqlx prepare check), each verbose.
- A note that the App Store exception file (`COPYING.iOS`) waits for the owner's wording.

### Excluded
- `COPYING.iOS` text (owner, R18) and its CI check (enabled when the text exists).
- Install guide, operator documentation, release notes (later epics).
- Web and mobile commands (added by E-26 and E-27 when those projects exist).

## Acceptance Criteria

**AC-1: Licence text is exact**
Given the repository root
When the licence file is compared with the FSF AGPL-3.0 text
Then it is identical (FT-P-74 step 2, licence part; RESTRICT-29)

**AC-2: README matches the tree**
Given the README layout section and the repository
When each folder the section names is checked
Then every named folder exists, and every top-level folder of the repository is named

**AC-3: Local gate commands run as written**
Given a clean checkout of the skeleton
When the PR-gate commands of the README run in the stated directory
Then each exits 0 and none uses a quiet flag

**AC-4: No placeholder exception file**
Given the repository root
When it is listed
Then no `COPYING.iOS` file exists, and the README states that it waits for the owner's text

## Non-Functional Requirements

**Compatibility**
- Plain Markdown; no generated content.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Checksum of the licence file against the FSF text | Equal |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Repository root | FT-P-74 step 2 (licence text present) | AGPL-3.0 text present; the exception-file part stays NOT-RUN until the owner supplies it | — |
| AC-3 | Clean checkout | README commands | All exit 0 | — |

## Constraints

- ADR-011 (AGPL with App Store exception), RESTRICT-29.

## Risks & Mitigation

**Risk 1: README drifts from the tree**
- *Risk*: later epics add folders without updating the section.
- *Mitigation*: AC-2 is a check the reviewer repeats when a top-level folder is added.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `COPYING.iOS` wording is missing (E-00 risk 3, R18); FT-P-74 step 2 cannot fully pass until it exists. | Project owner | open | Medium |
| `ci_cd_pipeline.md` lists `cd web` in the local checks; with the `src/` root this becomes `cd src/web` once E-26 creates it. | E-26 owner | open | Low |
