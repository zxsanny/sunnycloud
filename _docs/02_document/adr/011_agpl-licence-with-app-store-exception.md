# ADR-011: AGPL-3.0 licence with an App Store exception

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

The project ships a server, workers, desktop daemons and mobile apps, and publishes a worker image that contains ffmpeg with libx264, which are GPL. Draft 01 said "ship ffmpeg with x264, so the project is GPL-compatible" (decision D3). The second research round found that reasoning wrong: ffmpeg runs as a subprocess, and the GPL attaches to the ffmpeg layer, not to the Rust code, so the project licence is a free choice (`00_research/raw/b_ops_licence_scope.md` finding F3). The owner chose AGPL-3.0. The Apple App Store terms conflict with GPL-family licences, and precedent (Nextcloud iOS, Signal-iOS, Ente) is to ship on the store with an explicit exception from the copyright holders (finding F4).

- Acceptance criteria addressed: AC-63 (SBOM, signed images, weekly rebuild, 0 known HIGH/CRITICAL CVEs in the media libraries), AC-22 (a working install in 30 minutes needs a published, pullable worker image)
- Restrictions addressed: RESTRICT-29 (AGPL-3.0, App Store exception file, GPL ffmpeg/x264 with source offer, Slint GPL-3.0 compatible, licence file before the first outside contribution), RESTRICT-13 (no `libfdk_aac`, no nonfree), RESTRICT-12 (`cargo deny` blocking), RESTRICT-30 (build and release pipeline)
- Risks addressed: R18 (store policy, App Store plus AGPL needs consent of all copyright holders, no GPL crates in the iOS binary), R20 (GPL source offer for weekly images, codec patents), R19 (Windows signing needs the licence file)
- Research source: `_docs/00_research/raw/b_ops_licence_scope.md` § Findings table (F3, F4, F5) and § Licence matrix

The decision is non-trivial because three obligations interact: the network clause for operators, the GPL source offer for a published image, and the store exception, which only works if every copyright holder has agreed to it.

## Decision

We will license the server, workers, desktop daemons and apps under AGPL-3.0, add an App Store exception file, and meet the GPL obligations of the worker image with a published source offer.

Repository root holds `LICENSE` (AGPL-3.0) and `COPYING.iOS` (the exception: the copyright holders do not enforce the store-terms conflict); CI fails the `supply-chain` job when either is missing. The decoder image is built with `--enable-gpl --enable-libx264` and without `--enable-nonfree`; every build, including each weekly rebuild, publishes `decoder-sources-<version>-<yyyymmdd>.tar.zst` (tarballs, patches, recipe) and points to it from `/usr/share/doc/sunnycloud-decoder/SOURCE-OFFER.txt` and the OCI label `org.sunnycloud.source-offer`, plus `snapshot.debian.org` URLs of the exact Debian sources (x264 and others). The README carries the HEVC/codec patent note. Slint is used under its GPL-3.0-only option, which GPLv3 section 13 allows to combine with AGPL.

The iOS binary must contain no GPL-only crates beyond what the exception covers: `cargo deny check advisories bans licenses sources` runs on every PR with an allowlist for the iOS build. Before the first outside contribution the project adds a contribution mechanism; R18 names a DCO from the first outside contribution.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| MIT or Apache-2.0 | Compatible with every dependency in the licence matrix, so this is a policy rejection by the owner: no network copyleft, a hosted fork could stay closed |
| GPL-3.0 | No obligation for operators who modify and host the server (no network clause); draft 01 listed "GPL-3.0 or AGPL-3.0" and the owner chose AGPL (`06_component_fit_matrix.md` D3 and Round 2 changes) |
| Source-available (BSL-style) | Not OSI-approved: SignPath Foundation Windows signing is free only for OSI-licensed OSS and needs an OSI licence (`ci_cd_pipeline.md` D-1); also leaves Slint's GPL-3.0-only route unusable for the desktop apps |
| Proprietary | Same two blockers, plus self-hosters expect inspectable code; Slint would need its paid or royalty-free terms (`mve_round2.md` Slint licensing) |

## Consequences

### Positive

- Operators and forks must publish changes to a hosted service; the project is consistent with Nextcloud, Signal-iOS and Ente precedent.
- One licence covers the Rust code, the apps and the Slint settings window; SignPath eligibility is kept (R19).
- The source offer per build, weekly included, closes the gap a stale tarball would leave (R20 mitigated).

### Negative

- The exception depends on the consent of every copyright holder; one outside contribution without it can force a store pull, as VLC did in 2011 (R18, High).
- Some companies refuse AGPL, so adoption by them is closed.
- Every weekly rebuild produces a source tarball to store and attach, and anyone who republishes the image takes the same duty.
- A future proprietary build is not possible without relicensing or replacing Slint's GPL route.

### Neutral / Open

- Whether a DCO is enough or a CLA is needed to cover the exception is not decided; R18 says DCO, but the exception grants rights beyond the licence, so the owner should confirm with legal advice before accepting any outside contribution.
- HEVC decode patents in the shipped libheif and libde265 stay a distributor risk with no enforcement evidence found (R20, Low).
- Apple and Google developer accounts are open owner items (D-5).

## Evidence

- `_docs/00_problem/restrictions.md` § Hosting, licence and operations
- `_docs/02_document/tests/restriction-ids.md` § Hosting, licence and operations (RESTRICT-29)
- `_docs/02_document/deployment/ci_cd_pipeline.md` § PR gate (`ci.yml`) (`supply-chain` row); § Release artefacts; § Owner decisions this document depends on
- `_docs/02_document/deployment/containerization.md` § Images (Decoder: ffmpeg build; build checks; source offer)
- `_docs/02_document/deployment/deployment_procedures.md` (release checklist: source offer, `LICENSE`, `COPYING.iOS`)
- `_docs/02_document/risk_mitigations.md` § Risk Register (R18, R19, R20)
- `_docs/00_research/06_component_fit_matrix.md` § Decisions (D3); § Round 2 changes (Licence)
- `_docs/00_research/02_fact_cards/mve_round2.md` § MVE — Slint 1.18.1 (Licensing)

## Notes

Add the Slint GPL text to the third-party notices. Re-evaluate the contribution mechanism (DCO or CLA) and the exception wording with legal advice before the first outside contribution and before the first App Store submission in v1.1.
