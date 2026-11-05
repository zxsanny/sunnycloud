# sync-core and sync-core-wasm skeletons with the wasm32 split

**Task**: AZ-3739_sync_core_skeletons
**Name**: sync-core and sync-core-wasm skeletons
**Description**: Prove the native/WASM split of the client core before E-23 fills it: the WASM adapter builds and packages for the browser with no tokio, no reqwest and no dependency on the native client crate
**Complexity**: 2 points
**Dependencies**: AZ-3652_initial_structure
**Component**: 13 sync_core (bootstrap part)
**Tracker**: AZ-3739
**Epic**: AZ-3621 (E-00)
**Ref**: E-00/c07

## Problem

The web app must run the same hashing and commit-ID rules as the native clients through a WASM module (ADR-001), and that module must never pull the native executor (tokio, reqwest). If the split is not fixed before E-23 adds code, the first native dependency that leaks into the WASM crate breaks the browser build late and expensively. AZ-3652 already creates both crates empty and builds them for `wasm32-unknown-unknown`; nothing yet proves that the WASM crate can be packaged for a browser or that its dependency tree stays clean.

## Outcome

- The WASM adapter crate is a wasm-bindgen library that the pinned wasm-pack 0.15.x turns into a loadable browser package.
- A check fails whenever the WASM crate's dependency tree for `wasm32-unknown-unknown` contains tokio, reqwest, mio or the native client crate, or when wasm-bindgen appears in any other crate.
- The crate boundary and the home of the six client traits are recorded in the contract, so E-23, E-24 and E-27 build on one plan.

## Scope

### Included
- The wasm-bindgen setup of the WASM adapter crate with `wasm-bindgen =0.2.130` from the workspace pins, built as a library a browser can load; no exports beyond what wasm-bindgen generates.
- The browser package build with wasm-pack 0.15.x and the command to run it locally.
- The dependency-split check (WASM tree clean; wasm-bindgen only in the WASM crate) as an automated test.
- The native client crate stays an empty library that builds natively.
- The contract file for the two crates.

### Excluded
- The six client traits (`SyncApi`, `CredentialProvider`, `FileSource`, `DownloadSink`, `CheckpointStore`, `ProgressSink`): declared by E-23 (host traits row and executor rows), because their types live in `sync-core-proto`, which E-23 creates (AZ-3652 re-scope).
- `sync-core-proto` itself, the UniFFI feature, any export of the WASM module (E-23).
- Adding the wasm32 build to the PR gate (E-00/c09 CI skeleton).
- The web app's loader and Worker (E-26).

## Acceptance Criteria

**AC-1: The WASM adapter packages for a browser**
Given the pinned toolchain with the `wasm32-unknown-unknown` target and wasm-pack 0.15.x
When the WASM adapter crate is built with `--locked` for `wasm32-unknown-unknown` and packaged with wasm-pack
Then both steps exit 0 and the package contains the generated JavaScript glue and one `.wasm` file

**AC-2: The WASM dependency tree stays clean**
Given the workspace
When the dependency-split check reads the dependency tree of the WASM adapter crate for `wasm32-unknown-unknown`
Then it finds no tokio, reqwest, mio or native client crate, and a seeded dependency from the WASM crate on the native client crate makes the check fail

**AC-3: wasm-bindgen has one home**
Given the workspace manifests
When the dependency-split check lists the crates that depend on wasm-bindgen
Then the WASM adapter crate is the only one, and a seeded wasm-bindgen dependency in another crate makes the check fail

**AC-4: No placeholder code, no server edge**
Given both client crates
When the AZ-3652 crate-graph test and a search for `todo!(` and `unimplemented!(` run
Then the crate-graph test passes (no edge to `contracts` or a server crate) and the search finds 0 hits

## Non-Functional Requirements

**Compatibility**
- Builds with the workspace MSRV (1.88) and `--locked`; the package build runs in under 2 minutes on a GitHub-hosted runner.

**Reliability**
- Build and check commands print their progress (no quiet flags).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Dependency-split check on the real tree and on a seeded native edge | Pass on the real tree, fail on the seeded edge |
| AC-3 | wasm-bindgen ownership check on the real manifests and a seeded second user | Pass, then fail |
| AC-4 | Crate-graph test (AZ-3652) with both client crates present | Pass |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Clean checkout, pinned toolchain | wasm32 build and wasm-pack package of the WASM adapter | Both exit 0; glue and one `.wasm` present | Compatibility |

## Constraints

- One Cargo workspace (ADR-001); client traits live in the client crate, never in `contracts` (ADR-015).
- `wasm-bindgen =0.2.130` exactly, wasm-pack 0.15.x (E-00 pins).

## Risks & Mitigation

**Risk 1: A transitive dependency drags tokio into the WASM build later**
- *Risk*: E-23 adds a helper or crate whose default features pull tokio.
- *Mitigation*: the dependency-split check runs in `cargo test` on every PR, so the PR that adds the edge fails.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The E-00 row title says "with client traits", and the planned 3 points included them; AZ-3652 moved the traits to E-23 because their types live in `sync-core-proto`. This task is sized at 2 points for what remains. | E-00 owner | open | Low |
| The trait registry in the contract repeats names from 13 §2; if E-23 renames a trait, the contract needs the same change. | E-23 owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/sync-core/crate_skeleton.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
