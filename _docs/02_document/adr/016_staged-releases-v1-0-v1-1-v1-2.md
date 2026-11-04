# ADR-016: Staged releases v1.0, v1.1 and v1.2

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

The product spans a server, a web app, three mobile and desktop families, media workers, an optional end-to-end encrypted vault and four sign-in providers. The scope list in `restrictions.md` already splits it into three releases; this ADR records that split as a decision with its reasons and its fallback, because three things push against shipping everything at once. The schedule risk is the largest in the register (R15, scored Critical: 21 components, about 50 ports, real dependencies in tests). The Apple platforms need developer accounts, notarisation, the App Store guideline 4.8 rule (Google or GitHub sign-in on iOS only when Sign in with Apple is enabled) and consent from all copyright holders for the AGPL exception (R18). The vault changes its format if the external review finds problems, and `aead-stream` is unaudited (R23).

Acceptance criteria carry a release tag in `acceptance_criteria.md`, so the split also decides which tests gate which release. Staging has to keep v1.0 useful on its own: an operator with Android phones, a web browser and Linux or Windows desktops gets sync, camera backup, share links and CPU transcoding.

- Acceptance criteria addressed: AC-28 (NVENC opt-in, v1.1), AC-67 (macOS folder-access denial, v1.1), AC-13 and AC-14 and AC-60 (vault, v1.2), AC-9 (Android in v1.0, iOS in v1.1), AC-46 (Android reinstall, v1.0)
- Restrictions addressed: RESTRICT-01 (v1.0 scope), RESTRICT-02 (v1.1 scope), RESTRICT-03 (v1.2 vault only after external review), RESTRICT-04 (out of scope list), RESTRICT-14 (NVENC off by default, v1.1), RESTRICT-21 (iOS sign-in rule), RESTRICT-27 (iOS background work is best effort)
- Risks addressed: R15 (complexity and schedule), R18 (store and distribution policy), R23 (vault format change after review)
- Research source: `_docs/01_solution/solution.md` § Staged roadmap

## Decision

We will ship in three releases. **v1.0**: the server (API, workers with CPU transcoding, Caddy), the Svelte 5 web app, the Android app, the Linux and Windows desktop agents, sign-in with GitHub and any generic OIDC provider (Google through OIDC), encryption at rest, deletion safety, backup, restore and upgrade. **v1.1**: iOS, the macOS desktop agent, Sign in with Apple, opt-in NVIDIA NVENC encoding and Live Photo pairing. **v1.2**: the opt-in end-to-end encrypted vault, released only after an external review of the vault format and crypto. Facebook sign-in and Intel, AMD and Apple GPU encoding are out of scope (the latter documented as experimental only).

Staging is enforced in the structure, not only the calendar. The vault is opt-in and lives in separate crates (`vault_core`, `vault_service`) that v1.0 and v1.1 do not ship; the `vault: bool` flag exists on `PublishEntry` and is false in v1.0, and `revisions.client_meta` and `format_version` are kept so the v1.2 migration is additive. The iOS app reuses the `mobile_backup` crate and the shared `sync-core`; the platform clients differ only in thin native layers (UniFFI with Kotlin or Swift owning the uploader handle).

Contingency (R15): if the first vertical slice (upload, commit, feed, download) slips by more than 25%, the Linux and Windows desktop agents move to v1.0.1 and v1.0 ships server, web and Android first. The `sync-core` crate and the protocol are shared, so nothing is reworked. Contingency (R18): Android ships through the sideload or F-Droid channel while Play review runs, and the desktop MSI ships unsigned with a documented warning if signing is not ready (ADR-017). Contingency (R23): if the review changes the vault format, v1.2 slips and v1.0 and v1.1 are unaffected.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| One big release with all platforms and the vault | The Critical schedule risk (R15) gets a single failure point: a slip in the vault review or Apple onboarding would hold back 100% of the already-working server, web and Android; 3 releases bound each slip to its own scope |
| Fewer platforms (server, web and Android only; desktop later) | Folder sync on Linux and Windows desktops is a headline use case (AC-5, AC-35 are device-convergence targets); dropping it by default removes more value than the contingency does. The 25% rule keeps it as a fallback, not the plan |
| Vault in v1.0 | RESTRICT-03 requires an external review first, and the unaudited `aead-stream` plus the key-wrap decision (R35) make a format change likely (R23, probability High). Shipping an irrevocable on-disk format before review turns a review finding into a migration of every user's vault |
| iOS and macOS in v1.0 | Needs Apple Developer and notarisation setup, App Store AGPL consent from all copyright holders and the 4.8 sign-in rule (R18, Open); none can be closed by engineering alone |

## Consequences

### Positive

- v1.0 stands alone as a complete product for Android, web, Linux and Windows users, and each later release adds platforms or features without changing the sync protocol (`/api/v1`, additive-only).
- Review and store work for the high-uncertainty items (vault audit, Apple accounts, Play pre-review) runs in parallel with v1.0 instead of blocking it.
- Release tags on every acceptance criterion tell the test plan which tier gates which release.

### Negative

- iOS users and Mac desktop users wait for v1.1; the iOS app exists as a stub in v1.0 documents, which costs reading effort.
- Several AC wordings carry two release tags (for example AC-9 for Android in v1.0 and iOS in v1.1), so a release gate has to filter by tag.
- The staging constraint (vault crates not shipped in v1.0) requires keeping unused v1.2 hooks (`vault` flag, `format_version`) compiled and tested in v1.0.
- Contingency v1.0.1 splits the desktop test matrix across two releases if it triggers.

### Neutral / Open

- Dates are not fixed here; the staging order is.
- Open owner items that touch the split: external review of blob encryption before v1.0 (R08), Windows signing and the Apple and Google accounts (R18, R19), and vault share links beyond v1.2.

## Evidence

- `_docs/00_problem/restrictions.md` § Releases
- `_docs/02_document/tests/restriction-ids.md` § Releases (RESTRICT-01 to RESTRICT-04)
- `_docs/01_solution/solution.md` § Staged roadmap
- `_docs/02_document/architecture.md` § Component list (18 ios_app, 19 vault_core, 20 vault_service); § 1. System Context (External systems); § 8. Key Architectural Decisions
- `_docs/02_document/risk_mitigations.md` § Risk Register (R15, R18, R23); § Detailed Analysis (Critical and unresolved High): R15, R18, R23
- `_docs/02_document/components/02_tree/description.md` § TreeCommit (owns the commit transaction) (`vault: bool`, false in v1.0)
- `_docs/00_problem/acceptance_criteria.md` AC-9, AC-28, AC-67, AC-13, AC-14, AC-60

## Notes

Triggers to revisit: the first-slice slip check (25% over plan) decides v1.0.1 for the desktop agents; the external vault review outcome decides the v1.2 date. `architecture.md` § 8 lists this decision as "ADR-006" in a provisional list; the file number here is 016 and the list should be updated to match.
