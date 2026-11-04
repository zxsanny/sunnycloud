## Loop-end channel (ordinary product loops)

loop_end_merge: stage+main

# sunnycloud — CI/CD Pipeline

**Status**: draft (planning, no code yet)
**Date**: 2026-11-04
**Sources**: `architecture.md` §1, §3; `restrictions.md` (Hosting, licence and operations; dependency floors); `tests/environment.md` (tiers, runners, timeouts); AC-22, AC-24, AC-25, AC-43, AC-63, AC-64, AC-65.

The section above is the loop-end setting from `_docs/04_deploy/ci_cd_pipeline.md`, kept verbatim. That file stays the stub the autodev flow reads; this file is the planning document that the `/deploy` step later turns into workflows.

## Principles

- **One canonical path.** All logic lives in repository scripts (`scripts/ci/*.sh`, `tests/run.sh`). GitHub Actions and Woodpecker only check out, restore caches, call the same scripts and upload artefacts. A job that cannot be expressed as a script call is a smell.
- **Platform**: GitHub Actions on `https://github.com/zxsanny/sunnycloud` is primary. Fallback: Forgejo `git.azaion.com` with Woodpecker `ci.azaion.com` (remote not yet configured, see `infra_topology.md`). The fallback only builds and tests: it carries the PR gate (`core`, supply chain, image build); it never signs, never pushes release or `stage`/`main` tags and never creates a release. Self-hosted-runner tiers run there only if a matching Woodpecker agent label exists.
- **Pinning (RESTRICT-30)**: every `uses:` is a full commit SHA with a version comment; every base and service image is a digest; toolchains are pinned (`rust-toolchain.toml`, `.nvmrc`, Flutter version file). Dependabot (ecosystems `github-actions`, `docker`, `cargo`, `npm`, `pub`, daily) raises the bumps; a bump PR runs the full PR gate.
- **No quiet test runs.** Test commands print progress (`cargo test` without `-q`; `tests/run.sh` verbose). Any step without new log output for 2 minutes is reported STALLED and aborted by `scripts/ci/watchdog.sh` (30 s tick), per the long-running-jobs rule and `tests/environment.md`.
- **Least privilege**: workflow default `permissions: contents: read`; jobs that push images get `packages: write`, signing gets `id-token: write`, and only the `release` job runs in the protected GitHub Environment `release` (required reviewer = owner) that holds signing material.
- **Self-hosted runners** (REF-SRV, KVM, lab controller, owner Mac) run only on `push` to `dev`, `stage`, `main`, tags, `schedule` and `workflow_dispatch`. Never on `pull_request` from forks. Runners are ephemeral or wiped per job.

## Branches and triggers

| Branch / event | What runs | Result |
|----------------|-----------|--------|
| PR to `dev` | `ci.yml` PR gate (below) on GitHub-hosted runners | Required checks |
| push `dev` | PR gate + image build (not pushed except `sha-` tags to GHCR) | |
| push `stage` (fast-forward from `dev` at loop close) | PR gate + image build and push `stage-<sha>`, sign, SBOM, ops drill | Staging artefacts. No deploy: no remote deploy target exists |
| push `main` (fast-forward from `stage`) | Same as `stage` plus `main-<sha>` tags; release candidate material | |
| nightly 02:00 UTC | `nightly.yml`: `perf`, `web` (50 GiB), `android-emu`, `desktop` | Reports in `e2e-results/` artefacts |
| weekly Monday 03:00 UTC | `weekly-rebuild.yml`: decoder and worker images | AC-63 |
| tag `v*` on `main` | `release.yml` | Release artefacts |
| `workflow_dispatch` | Any workflow, plus the lab matrix | Release-candidate lab runs |

**Loop-end merge (`stage+main`)**: after smoke PASS and the loop-close push of `dev`, the agent fast-forwards `stage`, then `main`, on the CI remote and watches the resulting runs. Never a force-push; a non-fast-forward remote is a named blocker. Merging other branches and tagging need an explicit ask (git-workflow rule).

## PR gate (`ci.yml`)

Jobs run in parallel unless `needs:` is shown. Tier codes are those of `tests/environment.md`.

| Job | Content | Runner | Gate |
|-----|---------|--------|------|
| `rust-lint` | `cargo fmt --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `SQLX_OFFLINE` drift check (`cargo sqlx prepare --check`) | GitHub-hosted ubuntu-24.04 | 0 errors |
| `rust-test` | `cargo test --workspace --locked` against a **PostgreSQL 18 service container** (digest-pinned); coverage by `cargo llvm-cov` | ubuntu-24.04 | all pass; 75% overall, 90% critical paths |
| `pg18-migrations` | Apply all migrations to an empty PG 18; apply to the previous-release schema snapshot; kill the migration mid-way and assert rollback and refusal to start (AC-65) | ubuntu-24.04 | pass |
| `web` | `npm ci`, `svelte-check`, lint, `vitest`, build; OpenAPI client generated and diffed | ubuntu-24.04 | pass, 0 drift |
| `openapi` | Lint the spec; generate the two-account IDOR matrix and the CSRF matrix; fail when an operation is not covered (AC-58, AC-59) | ubuntu-24.04 | pass |
| `mobile` (path filter) | `flutter analyze`, `flutter test`, Android build (`assembleDebug`), NDK r28+ 16 KB alignment check, UniFFI bindings drift | ubuntu-24.04 | pass |
| `desktop-build` (path filter) | Linux and Windows daemon + settings builds; no signing | ubuntu-24.04, windows-2022 | build succeeds |
| `supply-chain` | `scripts/ci/audit.sh` (below), `cargo deny check advisories bans licenses sources`, npm and pub licence scans, `LICENSE` and `COPYING.iOS` present | ubuntu-24.04 | 0 findings without a valid ignore |
| `images` | Build api, worker, decoder and the `-test` tags of api and worker (BuildKit, layer cache in GHCR); `check-decoder-image.sh`; the release api and worker images return 404 for both `POST /test/clock` and `GET /internal/v1/test/clock-offset` (this early check is repeated on the pushed digest in `release.yml`) | ubuntu-24.04 | pass |
| `core` (needs `images`) | `tests/run.sh --profile core` with the `-test` api and worker images, one API process, the interactive and general decoder containers, short configured durations, shaper on | ubuntu-24.04 (4 vCPU, 16 GB) | all FAIL-free; timeout 45 min |
| `ops-drill` (needs `images`) | Backup-restore drill (dump, wipe, restore, `admin epoch-bump`, `scrub run --full`, `doctor` including the `.epoch` check) and upgrade from the previous release images (AC-65, AC-41), `tests/run.sh --suite ops` | ubuntu-24.04 | pass; before the first release the "previous" is the last release candidate, never skipped silently: `NOT-RUN(NO-PREV-RELEASE)` is recorded |

The GitHub-hosted disk (about 14 GiB free) rules out `blobs-large`; large-file work runs on the self-hosted tier.

**Dependency floors and ignores (AC-63, restrictions)**
- `deny.toml` bans `tokio < 1.54.1`, `h2 < 0.4.16`, `bytes < 1.11.1`, `rustls < 0.23.45`, sets `yanked = "deny"` (tokio 1.54.0 was yanked), allows only GPL-3.0-compatible licences, and restricts sources to crates.io.
- `deny.toml` is the single place for advisory ignores. Each entry carries `reason` text with `added=YYYY-MM-DD; expires=YYYY-MM-DD; why=...`. `scripts/ci/audit.sh` (1) fails if any entry lacks the fields, is past `expires`, or has `added` older than 90 days, (2) derives the `--ignore` list for `cargo audit` from the same entries, (3) runs `cargo audit --deny warnings` and `cargo deny check`. Both tools are blocking steps; there is no second ignore list.
- `libheif >= 1.22.0` is asserted in the decoder image check, not by cargo.

## Image build, signing, SBOM and scanning (AC-63)

Runs in `images` (build only) and in `stage`/`main`/`release` (push and sign).

| Step | Tool | Rule |
|------|------|------|
| Build | `docker/build-push-action` (SHA-pinned), BuildKit, `provenance: mode=max` | Bases by digest; `--locked` Cargo; `SOURCE_DATE_EPOCH` set from the commit |
| SBOM | Syft, CycloneDX, **merged with** `components.cdx.json` from the decoder (source-built ffmpeg, libheif, libvips) | Attached to the image as a cosign attestation and as a release asset |
| Scan | Trivy (OS and language packages) **and** Grype on the merged SBOM (catches source-built components by CPE) | Fail on any HIGH or CRITICAL with a fix available, any image. For the decoder fail on **any** HIGH or CRITICAL in ffmpeg, libheif, libvips, libwebp, with or without fix, unless a dated ignore exists (same format as `deny.toml`) |
| Scanner canary | `scripts/ci/scan-canary.sh` scans a fixture pinning a deliberately old libheif and expects a finding | Proves the scanner sees source-built libraries; weekly and on scanner bump |
| Sign | `cosign sign` keyless (GitHub OIDC, Sigstore), digest only, never a tag | Verification command printed in the install guide: `cosign verify --certificate-identity-regexp 'https://github.com/zxsanny/sunnycloud/.github/workflows/release.yml@refs/tags/v.*' --certificate-oidc-issuer https://token.actions.githubusercontent.com <image>@<digest>` |
| Push | GHCR `ghcr.io/zxsanny/sunnycloud-{api,worker,decoder}`; `GITHUB_TOKEN` | Registry choice is an owner decision (below) |

**Fallback (Forgejo/Woodpecker)**: builds and tests only (owner decision, Step 4). Woodpecker cannot obtain a GitHub OIDC identity and holds no signing key, so there is one trust root: images and releases are signed only by the GitHub `release.yml` workflow. If GitHub is unavailable, a release waits.

**Weekly rebuild (`weekly-rebuild.yml`)**: rebuilds `decoder` and `worker` from the pinned base digests with `--no-cache --pull` and `apt-get dist-upgrade`, so Debian security fixes land without waiting for a release; runs `check-decoder-image.sh`, scan, SBOM, signing; pushes `:<semver>-<yyyymmdd>` and moves `:<semver>`. **Tag handling**: the immutable dated tag is the record; the moving `:<semver>` tag is what the release compose references for the decoder (the one image not pinned by digest, by design). Each rebuild also builds the matching `decoder-sources-<semver>-<yyyymmdd>.tar.zst` (tarballs, patches, recipe: the GPL source offer for exactly that build), uploads it as an asset of the existing GitHub Release `v<semver>` and points the image label `org.sunnycloud.source-offer` at it; a rebuild whose source asset upload fails is not pushed. If a HIGH or CRITICAL in ffmpeg, libheif, libvips or libwebp is found, the job fails and opens an issue; fixing means bumping a version in `deploy/docker/versions.env` through a normal PR. A release is blocked when the newest decoder build is older than 7 days (the release job rebuilds it first, so the rule holds by construction). Image creation time is in the OCI labels so an operator can see it.

## Tier to runner mapping (from `tests/environment.md`)

| Tier / code | Runs on | When | Timeout |
|-------------|---------|------|---------|
| Unit | Developer workstation and GitHub-hosted runner in `rust-test`, `web`, `mobile` | Every PR | job default 30 min |
| `C` black-box | GitHub-hosted ubuntu-24.04 (`core`) or any Docker host | Every PR and push | 45 min |
| `C-LARGE` | Self-hosted REF-SRV runner (`[self-hosted, ref-srv]`, 200 GiB XFS loop image) | Nightly and release candidate | 8 h |
| `C-REF`, perf, resource-limit | REF-SRV + REF-LOAD (`[self-hosted, ref-srv]`, `[self-hosted, ref-load]`); transcode timing on the REF-CPU4 cpuset | Nightly and release candidate | 4 h |
| `web` (Playwright, 50 GiB) | REF-SRV (`browsers` profile) | Nightly | 8 h |
| `EMU` | KVM-capable runner (`[self-hosted, kvm]`; GitHub's 4-vCPU Linux runners expose `/dev/kvm` on public repositories, verify before relying on it) | Nightly | 3 h |
| `desktop` containers | ubuntu-24.04 or REF-SRV | Nightly | 2 h |
| `H-LIN`, `H-WIN`, `H-ANDROID` | Lab controller (`[self-hosted, lab]`), `tests/lab/run-lab.sh --target ...`, VM snapshot revert first | Release candidate, `workflow_dispatch` | 12 h |
| `H-MAC`, Safari, macOS, iOS (v1.1) | Owner Mac (`[self-hosted, owner-mac]`) | Release candidate | 4 h |
| `H-GPU` (v1.1) | REF-NV | Release candidate | 2 h |
| `H-LIVE` | Owner accounts, manual | Release candidate | manual |
| `CI` (supply chain, licence, pinned actions, signature verification) | GitHub-hosted | Every PR; release | 30 min |
| `MAN` (AC-22 install timing) | Clean Ubuntu LTS VM, person with stopwatch | Release candidate | manual |

Results from any host other than the one the tier names are `NOT-RUN(HW-REF)` or the matching code, recorded in `_docs/_process_leftovers/`, never promoted to PASS. Gate: any FAIL blocks; NOT-RUN passes a release only for items the owner has waived with a reason.

## Release workflow (`release.yml`, tag `v*` on `main`)

1. Verify: tag is on `main`, `images.lock` and `versions.env` committed, newest `main` run green, nightly of the last 24 h green or each failure waived.
2. Rebuild decoder and worker fresh (weekly-rebuild steps), build api, worker, decoder from the tagged commit.
3. Gate: `core`, `ops-drill`, supply chain, scan, `check-decoder-image.sh`; fetch the latest lab-matrix results for the commit (lab runs are dispatched earlier; missing results block unless waived).
4. Push by digest. **Before signing**, start the pushed `api@digest` and `worker@digest` images and assert 404 for `POST /test/clock` and `GET /internal/v1/test/clock-offset` and the label `org.sunnycloud.test-clock=false`; a failure stops the release (the `images` job check is not enough, because it tests the build, not what was pushed). Then sign (keyless); attach SBOM attestations and provenance; write `images.lock` (image -> digest).
5. Build artefacts (below), sign them (cosign), upload as a **draft** GitHub Release together with an **unsigned** update manifest draft (version, per-OS SHA-256 of the uploaded payloads, proposed `serial` and `expires`). The workflow holds no updater key.
6. Environment `release` requires the owner's approval. The owner signs the manifest **offline** with the active updater key and attaches it to the draft; then the release is published.
7. Docs: release notes list breaking changes, `min_client_version`, migration notes, the Compose and Docker versions tested (these notes are what the upgrade procedure reads).

## Release artefacts

| Artefact | Built by | Signing / integrity | Status |
|----------|----------|---------------------|--------|
| Server images (api, worker, decoder) | `release.yml` | cosign keyless, SBOM, provenance; digests in `images.lock` and release notes | v1.0 |
| `sunnycloud-deploy-<ver>.tar.gz` (`compose.yml` pinning caddy, db, api, migrate and worker by digest from `images.lock`; the decoder is referenced by its moving `:<semver>` tag, see weekly rebuild; `Caddyfile`, `.env.example`, `secrets/README`, `backup.sh`, `restore.sh`) | `release.yml` | SHA-256 file, cosign blob signature | v1.0 |
| `decoder-sources-<ver>-<yyyymmdd>.tar.zst` (GPL source offer; one per decoder build, release and weekly) | `release.yml`, `weekly-rebuild.yml` | SHA-256 | v1.0 |
| `openapi.json` | `release.yml` | SHA-256 | v1.0 |
| Linux desktop: `.deb` (+ tar.gz) | `desktop-release` (ubuntu) | Listed in the offline-signed update manifest with its SHA-256 (AC-49); package signature optional | v1.0 |
| Windows desktop: MSI | `desktop-release` (windows-2022) | **Code-signing: open decision** (below); listed in the offline-signed update manifest with its SHA-256 | v1.0 |
| Android: AAB for Play, APK for sideload / F-Droid | `mobile-release` (ubuntu) | Play App Signing with an upload key held as an Environment secret; sideload APK signed with the same app key | v1.0 |
| macOS desktop (dmg, notarised), iOS (TestFlight / App Store) | `desktop-release`, `mobile-release` on macOS (owner Mac or macOS runner) | Developer ID + notarisation; App Store signing | v1.1 |

The **updater keys** (AC-49; update source fixed at build time and never the sync server; Step 4 decision S4-3): two Ed25519 key pairs, an *active* key and a *recovery* key, generated offline by the owner. Both public halves are compiled into the daemon. The manifest (`version`, per-OS entries with SHA-256, a monotonic `serial`, `expires`; exact layout is an ADR) is signed offline, or in a manual job the owner starts, and never by an automatic CI secret: no updater private key exists in GitHub or Woodpecker. The recovery key is kept in cold storage and signs only a manifest that rotates the active key (and a new build that pins the new pair). Loss of both keys means a one-time manual reinstall for every desktop user; the backup rule is the same as for the master key (two offline copies of each).

## Post-deploy polling (agent configuration)

| Key | Value |
|-----|-------|
| **enabled** | **no** |
| **long_running** | no |
| **poll_interval_seconds** | not applicable |
| **applies_after** | not applicable: no remote deploy target exists |
| **status_tool** | `gh run watch` (CI status only, on request) |
| **primary_gate** | `ci.yml` required checks |
| **report_to** | not applicable |

No remote deploy target exists: `infra_topology.md` lists no deploy host or registry, the architecture shows operator-run installs only, and staging is the `stage` branch artefacts, not a deployed service. Agents therefore report that CI was triggered and do not poll (`post-deploy-polling` rule: `enabled: no`). The gate file the agents read is `_docs/04_deploy/ci_cd_pipeline.md`; with no polling section there, polling is disabled by default. Copy this section into that file when `/deploy` runs. Change to `yes` (interval 30 s, tool `gh run watch`) when a shared staging host or a demo server exists. No "Loop production cut" section is defined for the same reason (default: ask).

## Local pre-push checks (agent)

These mirror the PR gate so the CI-parity gate can run them before an application-source commit or push.

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
bash scripts/ci/audit.sh
cargo deny check advisories bans licenses sources
cargo sqlx prepare --check --workspace
( cd web && npm ci && npm run check && npm test && npm run build )
bash scripts/ci/check-openapi-drift.sh
```
The `core` black-box tier needs Docker and runs through `tests/run.sh --profile core` when a change touches server behaviour.

## Caching and parallelism

| Cache | Key | Restore keys |
|-------|-----|--------------|
| Cargo registry and `target` (`Swatinem/rust-cache`, SHA-pinned) | `Cargo.lock` hash + toolchain | toolchain only |
| npm | `package-lock.json` | none |
| Docker layers | BuildKit cache in GHCR (`:buildcache` per image) | same image |
| Decoder media layer | `versions.env` + `ffmpeg.configure` hash | none (first build about 25 min) |

Parallel from the start: `rust-lint`, `rust-test`, `web`, `openapi`, `mobile`, `desktop-build`, `supply-chain`, `images`. `core` and `ops-drill` wait for `images`. A failure in any required job blocks the merge.

## Notifications

| Event | Channel |
|-------|---------|
| PR gate failure | GitHub check status (owner is the only recipient today) |
| Weekly rebuild or scan finding | GitHub issue labelled `security`, email from GitHub |
| Nightly red | GitHub issue labelled `nightly`, closed by the next green run |

## Owner decisions this document depends on

| # | Decision | Default used here |
|---|----------|------------------|
| D-1 | Windows code signing: SignPath Foundation (free for OSI-licensed OSS, publisher shown as SignPath Foundation, needs MFA and an OSI licence), a purchased OV/EV certificate, Azure Trusted Signing, or unsigned for v1.0 with SmartScreen warnings | SignPath application once the AGPL licence file exists; otherwise unsigned beta |
| D-2 | Image registry: GHCR only, or also Docker Hub and `git.azaion.com` registry | GHCR only |
| D-3 | Fallback signing key and whether fallback releases are allowed at all | Closed (Step 4): the fallback builds and tests only; no signing, no release |
| D-4 | Self-hosted runners: REF-SRV, REF-LOAD, KVM, lab controller, owner Mac exist and are registered | Not provisioned; tiers report `NOT-RUN` until they are |
| D-5 | Apple Developer (US$99/yr) and Google Play (US$25 once) accounts, privacy policy URL | Needed before v1.0 (Play) and v1.1 (Apple) |

## AC coverage

| AC | Pipeline element |
|----|------------------|
| AC-22 | `MAN` timed install on a clean VM at release candidate; release bundle contents |
| AC-24 | Scrub tests inside `core` (bit flip via `e2e-ctl`), throttle check in `perf` |
| AC-25 | `core` isolation inspection; image build flags checked by `check-decoder-image.sh` |
| AC-43 | `core` disk inspection; throughput part in `perf` |
| AC-63 | `supply-chain`, scan, SBOM, signing, weekly rebuild, canary, 90-day ignore rule |
| AC-64 | `core` old-version header test against `/api/v1`; `openapi.json` release asset |
| AC-65 | `pg18-migrations`, `ops-drill` on every `stage`/`main` push and every release |
