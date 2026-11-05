# contracts part 1: registries, lifecycle, 01 identity, 07 admin, 07a quota ports

**Task**: AZ-3768_contracts_part1
**Name**: contracts part 1 (registries, lifecycle, identity, admin, quota)
**Description**: Declare in the server-only `contracts` crate the late-registered list ports, the account lifecycle hook and every port of 01 identity, 07 admin and 07a quota, with their shared DTOs and typed errors, signatures only
**Complexity**: 5 points
**Dependencies**: AZ-3764_sc_error, AZ-3765_sc_clock, AZ-3766_sc_ids, AZ-3786_sc_observability, AZ-3652_initial_structure
**Component**: contracts (server ports)
**Tracker**: AZ-3768
**Epic**: AZ-3621 (E-00)
**Ref**: E-00/c03

## Document Dependencies

- `_docs/02_document/contracts/shared_error/sc_error.md` — `ErrorCode`, `BusinessError`, produced by E-02/c01.
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — `Timestamp`, produced by E-03/c01.
- `_docs/02_document/contracts/shared_ids_hashing/sc_ids.md` — `RawToken`, `UserCode`, produced by E-04/c01.
- `_docs/02_document/contracts/shared_db/sc_db_access.md` — `DbTx` (the AZ-3652 stub until E-06/c01a wraps it).

## Problem

ADR-015 makes every cross-component call go through a port trait in `contracts`; about 50 ports must exist before any component can be built or faked. Identity, admin and the quota ledger are called by almost every other component (commit guard, erase hooks, quota in the commit transaction), and the late-registered lists (janitors, erase hooks, blob-owner cleanup, doctor, health, readiness) are how the composition root breaks wiring cycles. Without these traits, components would call each other's types and the crate graph would cycle.

## Outcome

- Every port of part 1 exists as an object-safe trait, with the DTOs and error variants of the contract, and nothing else.
- Each component epic can build a recording double of any part-1 port and code against it.
- A listing test proves the part-1 names are all present.

## Scope

### Included
- Registries and lifecycle: `Janitor`, `AccountLifecycleHook` with `EraseProgress`, `BlobOwnerCleanup`, `DoctorCheck` with `Finding`; `HealthContributor` with `HealthItem` and `ReadinessCheck` with `ReadinessItem` are re-exported from `sc_observability`, which declares them (owner decision 2026-11-05).
- 01: `Authenticator`, `PrincipalRevalidator`, `AccountGuard`, `SignInService`, `SessionService`, `DeviceGrantService`, `InviteService`, `OperatorIdentityOps`, `AccountAdmin`, and `Principal`, `Credential`, `Scope`, `Role`, the sign-in, session and device DTOs.
- 07: `AdminService`, `AccountService`, `AccountDeletionService`, `DevSeed`, `Actor` and the admin views.
- 07a: `QuotaLedger`, `QuotaAdmin`, `QuotaView`, `Drift`.
- The conventions of the contract (async, object safe, `Send + Sync`, `&mut DbTx` only where listed, one error enum per port, no HTTP or sqlx types).
- The listing test (part-1 names; extended by parts 2 and 3) and compile tests: one recording double per trait, `Arc<dyn Port>` use, `DbTx` not constructible outside `sc_db`.

### Excluded
- Any implementation (component epics E-10..E-22, E-30).
- 02, 03, 05 ports (E-00/c04) and 04, 06, 08, 20 ports (E-00/c05).
- The HTTP mapping of error variants (E-21).

## Acceptance Criteria

**AC-1: Every part-1 port declared (E-00 AC 3, part 1)**
Given the contract's part-1 port list
When the listing test compares it with the public traits of `contracts`
Then 0 names are missing

**AC-2: Ports can be faked**
Given a recording double for each part-1 trait in the test suite
When the tests compile and run
Then each double is held as `Arc<dyn Port>`, shared across two threads, and records one call per method

**AC-3: Transactions are joined only where listed**
Given the part-1 signatures
When the compile tests run
Then exactly `AccountGuard::ensure_active`, the `AccountAdmin` mutators, the `QuotaLedger` methods, `AccountLifecycleHook::initialize` and `BlobOwnerCleanup::remove_owner_rows` take `&mut DbTx`, and a `compile_fail` doc test shows code outside `sc_db` cannot construct a `DbTx`

**AC-4: Signatures only, server-only crate**
Given the `contracts` crate
When it is searched for `todo!(` and `unimplemented!(` and the crate-graph test runs
Then the search finds 0 hits, `contracts` depends on no component crate, sqlx or HTTP crate, and no client crate depends on it (ADR-015)

**AC-5: Tokens are never printable (NFT-SEC-16)**
Given DTOs that carry a token (`SessionTokens`, `InviteToken`, `DeviceToken`, `DeviceAuthorization`, the setup token)
When each is formatted with `Debug`
Then the output contains a placeholder and none of the token bytes

**AC-6: Typed outcomes, no status codes**
Given the part-1 error enums
When the crate is searched for HTTP status numbers and status types
Then there are none; the variants match the contract tables exactly

## Non-Functional Requirements

**Compatibility**
- Builds with the workspace MSRV and `--locked`; clippy `-D warnings` clean.

**Reliability**
- The listing and compile tests run in `cargo test --workspace`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Listing test against the part-1 list | 0 missing |
| AC-2 | Recording doubles as `Arc<dyn Port>` across threads | Compile and record calls |
| AC-3 | `compile_fail` doc test constructing `DbTx` | Does not compile |
| AC-4 | Crate-graph test (AZ-3652) | Passes |
| AC-5 | `Debug` of token DTOs with known values | No token bytes |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| — | None: this task adds no runtime behaviour; the ports are exercised black-box by E-15, E-17, E-11 and the pipeline tasks | — | — | — |

## Constraints

- ADR-015 (ports only); module-layout Rule 6 (later changes are a `contracts` task in the provider's epic, editing only that provider's part).
- `contracts` imports layer-1 helper types only, and only `DbTx` from `sc_db`.

## Risks & Mitigation

**Risk 1: A wrong port boundary (R15)**
- *Risk*: a signature turns out wrong and every implementer must change.
- *Mitigation*: signatures copied from the component descriptions; the vertical slice (E-10..E-15) runs first and finds errors early; open points are listed in the contract.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The port signatures name helper value types (`Timestamp`, `RawToken`, `UserCode`, `ErrorCode`), so this task depends on E-02/c01, E-03/c01 and E-04/c01. AZ-3652's Implementation Order puts the contracts parts before the helpers; the real order is helpers E-02..E-04 first. The helper epics need only AZ-3652, so there is no cycle. | E-00 owner | open | Medium |
| `CycleStats`, `ProviderInfo`, `SessionInfo`, `DeviceInfo`, `InviteInfo`, `AccountInfo` and the `list_accounts` filter have no field lists in 01, 07 or 12. | Owners of E-15, E-17, E-22 | open | Medium |
| `ReadinessCheck` has no method list in any spec; the contract infers it from the `/readyz` body of E-09. | E-09 / E-21 owners | open | Low |
| Resolved by owner decision 2026-11-05: `HealthContributor` and `ReadinessCheck` (with their items) live in `sc_observability`; this crate re-exports them, so `sc_observability` no longer depends on `contracts` and this task depends on AZ-3786 | Owner | resolved | High |
| `DoctorContext` fields are derived from the `doctor` options of 12 §3. | E-22 owner | open | Low |
| `Role` is `user`/`admin` in data_model §4.1 but invites grant `member`/`admin` (01). | E-15 owner | open | Low |
| `AccountDeletionService::request` takes a principal, but 12 also uses it for `admin user delete` (operator); the `DevSeed` refusal has no variant name. | E-17 owner | open | Low |
| Ids are typed as `Uuid`; `AccountId` (helper 07) has no definition (see `sc_db_access.md` open point 1). | E-06 owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
