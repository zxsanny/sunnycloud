# CI skeleton: PR gate jobs, audit and watchdog scripts, SHA pins, coverage gate

**Task**: AZ-3761_ci_skeleton
**Name**: CI skeleton
**Description**: The GitHub Actions PR gate with the jobs rust-lint, rust-test, pg18-migrations, supply-chain and images, calling repository scripts only, with actions pinned by SHA, read-only default permissions, the dated-ignore audit script, the 30-second watchdog and the coverage gate
**Complexity**: 5 points
**Dependencies**: AZ-3740_migrations_setup, AZ-3741_docker_compose_skeletons, AZ-3744_license_readme, AZ-3652_initial_structure
**Component**: Bootstrap (CI)
**Tracker**: AZ-3761
**Epic**: AZ-3621 (E-00)
**Ref**: E-00/c09

## Problem

Every later task needs a PR gate that runs the same checks for everyone, blocks supply-chain risks from the first commit (RESTRICT-12, RESTRICT-30, AC-63) and never waits silently on a hung step. Without it, unpinned actions or a yanked tokio can enter before anyone looks.

## Outcome

- A pull request to `dev` (and a push) runs five required jobs that pass on the skeleton in under 15 minutes.
- Every action is pinned by full commit SHA, images by digest, default permissions are read-only, and a check fails on any unpinned reference.
- Advisory ignores live in one place with dated reasons; the audit script fails on missing, expired or older-than-90-days ignores.
- Any step that prints nothing for 2 minutes is reported STALLED and aborted.

## Scope

### Included
- The PR-gate workflow with jobs (PR to `dev`, push to `dev`): `rust-lint` (`cargo fmt --check`, clippy with `-D warnings`, the offline-data check of E-00/c10 with the PG 18 service), `rust-test` (`cargo test --workspace --locked`, never `-q`, PostgreSQL 18 service by digest, `cargo llvm-cov` with 75% overall and 90% on the critical-path list, which starts empty), `pg18-migrations` (the migration check of E-00/c10 on an empty PG 18), `supply-chain` (audit script, `cargo deny check advisories bans licenses sources`, licence file present, wasm32 build of `sync-core-wasm` and `sc_ids`, secret scan), `images` (build only: api, worker, decoder, and `-test` of api and worker, BuildKit cache).
- The audit script: validates every ignore entry of the deny configuration (`added`, `expires`, `why`; not past `expires`; `added` at most 90 days old), derives the `cargo audit` ignore list from the same entries and runs `cargo audit --deny warnings` and `cargo deny check`.
- The watchdog script: wraps long steps, ticks every 30 s, prints a heartbeat with elapsed time and output growth, reports STALLED and aborts after 2 minutes without output.
- Pinning check: every `uses:` is a 40-hex SHA with a version comment, every image a digest; workflow default `permissions: contents: read`; no `pull_request_target` with a fork checkout.
- Dependabot for `github-actions`, `docker` and `cargo` (daily).
- Caching keyed on the workspace lock file and the toolchain.

### Excluded
- Jobs `web`, `openapi`, `mobile`, `desktop-build`, `core`, `ops-drill` and the release, nightly and weekly workflows (later epics per AZ-3652).
- The Woodpecker fallback file (added when that remote exists).
- Branch-protection settings (a repository setting; owner action).
- The silent-catch grep gate, the SQL `now()` gate and the release-symbol check (E-02, E-03; they run inside the jobs above once those tasks land).

## Acceptance Criteria

**AC-1: PR gate green on the skeleton**
Given a pull request to `dev` with the skeleton
When the PR-gate workflow runs
Then `rust-lint`, `rust-test`, `pg18-migrations`, `supply-chain` and `images` all pass, and the slowest job ends within 15 minutes of the workflow start

**AC-2: Pinned and least privilege (NFT-SEC-37)**
Given the workflows and Dockerfiles
When the pinning check runs, and again on a branch that adds `actions/checkout@v4`
Then the first run reports 0 unpinned references, read-only default permissions and no fork checkout under `pull_request_target`, and the second run fails

**AC-3: Advisory gates (NFT-SEC-35 steps 1-5, AC-63)**
Given branches that add a dependency with a known advisory and no ignore, an ignore without `why` or dates, an ignore dated 91 days ago, an ignore dated 89 days ago, and pins of tokio 1.54.0, h2 below 0.4.16, bytes below 1.11.1 or rustls below 0.23.45
When `supply-chain` runs on each
Then every branch fails except the one with the 89-day ignore

**AC-4: No silent waits**
Given a seeded step that sleeps without output for 3 minutes under the watchdog
When the job runs
Then the log shows a heartbeat line at least every 30 s, the step is reported STALLED after 2 minutes and aborted, and the job fails

**AC-5: No quiet test runs**
Given the workflows and CI scripts
When they are searched for `cargo test -q`, `--quiet` and `pytest -q`
Then there are 0 hits, and `rust-test` output lists each test by name

**AC-6: Coverage gate enforced**
Given a branch where overall line coverage is below 75%
When `rust-test` runs
Then it fails and prints the measured value; with the skeleton it passes

**AC-7: Images built, not pushed, on a PR**
Given a pull request
When `images` runs
Then the five images build and nothing is pushed to a registry

**AC-8: Secret scan blocks a committed secret**
Given a branch that commits a fake private key
When `supply-chain` runs
Then the secret-scan step fails and names the file (NFT-SEC-38 step 1, git part)

## Non-Functional Requirements

**Performance**
- PR gate on the skeleton under 15 minutes on GitHub-hosted runners (E-00 NFR).

**Reliability**
- Every long step runs under the watchdog; no step can hang for more than 2 minutes without output.

**Security**
- No secret in workflow files; only jobs that push get `packages: write` (none in this task).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Audit script on fixture deny configurations (missing fields, expired, 91 days, 89 days) | Fail, fail, fail, pass |
| AC-2 | Pinning check on fixture workflows (tag, SHA without comment, SHA with comment) | Fail, fail, pass |
| AC-4 | Watchdog on a fixture command that is silent for 150 s | STALLED after 120 s, non-zero exit |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Skeleton PR | Full PR gate | 5 jobs green, < 15 min | Performance |
| AC-2 | Branch with `@v4` action | NFT-SEC-37 steps 1-4 | Pinning check fails; read-only defaults | Security |
| AC-3 | Branches per case | NFT-SEC-35 steps 1-5 | Only the 89-day ignore passes | — |
| AC-8 | Branch with a fake key | NFT-SEC-38 step 1 (git history scan) | Secret scan fails | Security |

## Constraints

- YAML only checks out, caches and calls repository scripts (ci_cd_pipeline § Principles); cargo steps run with `src/` as working directory.
- RESTRICT-12 floors and RESTRICT-30 pinning; quality thresholds: 75% overall, 90% critical paths.

## Risks & Mitigation

**Risk 1: PR gate exceeds 15 minutes as crates grow**
- *Risk*: cold caches and five jobs push the gate past the budget.
- *Mitigation*: jobs run in parallel; the cache is keyed on the lock file and toolchain; the budget is measured in AC-1 and reported per job.

**Risk 2: Advisory ignore rot**
- *Risk*: ignores stay forever.
- *Mitigation*: the 90-day rule fails the gate when an ignore ages out.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The secret scan (gitleaks, SHA-pinned) is required in the PR gate by `environment_strategy.md` but not listed in `ci_cd_pipeline.md` or E-00; AZ-3652 proposes it as a `supply-chain` step, which this task does. | Deploy owner | open | Low |
| NFT-SEC-35 step 6 (both jobs are required checks) is a GitHub branch-protection setting the owner configures. | Project owner | open | Low |
| `ci_cd_pipeline.md` lists `COPYING.iOS` in `supply-chain`; its check is enabled only when the owner supplies the text (E-00 risk 3). | Project owner | open | Low |

## Contract

This task has no new contract file; the jobs its consumers (E-03/c01, E-04/c01 and every later task) add steps to are those of `_docs/02_document/deployment/ci_cd_pipeline.md` § PR gate, as narrowed by AZ-3652 § CI/CD Pipeline.
