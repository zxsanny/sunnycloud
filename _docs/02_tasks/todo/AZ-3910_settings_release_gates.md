# Settings release gates in CI: dependency tree, size, no token over IPC, no secrets in logs

**Task**: AZ-3910_settings_release_gates
**Name**: Settings release gates
**Description**: CI checks on the release build of the settings window: no HTTP client and no femtovg or Skia in its dependency tree, the binary size recorded, no token in the IPC traffic of a signed-in session, and no user path or user code in INFO logs
**Complexity**: 2 points
**Dependencies**: AZ-3890_daemon_client, AZ-3895_settings_app_shell, AZ-3896_account_screen, AZ-3909_native_dialogs, AZ-3908_token_store_device_login, AZ-3917_daemon_control_impl, AZ-3761_ci_skeleton
**Component**: 15 desktop_settings
**Tracker**: AZ-3910
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c13

## Problem

The window must have no network access at all (15 §11) and must stay on the software renderer, because the GL path cost 160-420 ms and 121-136 MB (ADR-012). Both are properties of the dependency tree that can regress silently with one added crate. It must also never see a token (AC-48) and must not leak user paths or user codes into logs (15 §9). These are release gates, so they run in CI on every release build.

## Outcome

- CI fails when the release binary's dependency tree contains an HTTP client, femtovg or Skia.
- CI records the release binary size on every run.
- ST-15-2 (dependency and IPC parts) and ST-15-3 run in CI and pass.

## Scope

### Included
- The dependency-tree check of the release binary (`reqwest`, `hyper`, `ureq` or any other HTTP client; `femtovg`; `skia`), with planted fixtures proving that it fails.
- Recording the release binary size per CI run.
- ST-15-2 steps 1 and 3: dependency inspection, and an IPC capture of a signed-in session against the real daemon searched for the token value.
- ST-15-3: sentinel folder `/home/u/SECRET-FOLDER` and user code `WDJB-MJHT` through the folder and sign-in flows, then a search of the INFO log.

### Excluded
- ST-15-2 step 2, the offline run of the AT set (E-25/c14).
- The daemon's own token rules (E-24, NFT-SEC-34).

## Acceptance Criteria

**AC-1: No HTTP client, no GL renderer (ST-15-2)**
Given the release build of the settings binary, and fixture builds that add an HTTP client and femtovg
When the CI dependency check runs
Then the real build passes and each fixture fails with the offending crate named

**AC-2: Size recorded**
Given any CI run of the release build
When the job ends
Then the binary size is recorded in the job output and kept with the build's artifacts

**AC-3: No token over IPC (AC-48, ST-15-2)**
Given the real daemon signed in through the device grant and the window driving every screen
When the IPC traffic of the session is captured
Then the token value is found 0 times in the capture

**AC-4: No secrets in logs (ST-15-3)**
Given the sentinel folder `/home/u/SECRET-FOLDER` and the user code `WDJB-MJHT` used in the folder and sign-in flows
When the window's log at INFO is searched
Then both sentinels are found 0 times

**AC-5: Gate blocks**
Given a failing check
When the CI pipeline runs
Then the pipeline fails and the build is not released

## Non-Functional Requirements

**Security**
- The checks run on the same release profile that ships (fat LTO, `opt-level = "s"`, strip, `panic = "abort"`).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Matcher over a dependency list with and without forbidden crates | Fails only with forbidden crates |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Release build; fixture builds | Dependency check | Real passes; fixtures fail | ST-15-2 |
| AC-2 | CI run | Size record | Size in output and artifacts | |
| AC-3 | Real daemon, signed in | IPC capture | 0 token matches | ST-15-2, NFT-SEC-32 step 5 |
| AC-4 | Sentinels | INFO log search | 0 matches | ST-15-3 |
| AC-5 | Failing fixture | Pipeline | Fails; no release | |

## Constraints

- The checks run on the release binary, not a debug build.

## Risks & Mitigation

**Risk 1: A transitive dependency hides an HTTP client**
- *Risk*: a crate pulls an HTTP client under another name.
- *Mitigation*: AC-1 checks the full resolved tree, not the direct dependencies.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The epic row names a "size" gate but no spec sets a binary size limit; this task only records the size until the owner sets one | Owner / E-25 | open | Low |
