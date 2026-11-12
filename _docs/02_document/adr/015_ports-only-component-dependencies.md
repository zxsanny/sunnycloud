# ADR-015: Ports-only dependencies between components

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

The server has 21 components (01 to 20 plus 07a) in one Cargo workspace and about 50 port traits. The first drafts let components call each other's concrete types, and the evaluator pass found constructor-injection cycles between quota, tree, transfer, blobstore, identity and the feed, plus a commit path whose transaction owner was unclear (quota and feed were each updated from a different crate). Account deletion needed five separate admin traits (`UploadAdmin`, `ShareAdmin::revoke_all_for_account`, `MediaAdmin::cancel_jobs/purge_derived`, `TreeMaintenance::purge_account_batch`, `BlobAdmin::files_for_account`), and the first erase loop could not finish because sessions, tokens, grants, login codes and receipts were left behind.

This is non-trivial because the same decision has to hold for the commit transaction (AC-69, AC-74), account erase (AC-27), the media queue (08 versus the worker), the HTTP delivery layer and the clients, which cannot depend on server-only crates. Complexity is itself the top schedule risk (R15), so the rule has to remove coordination cost, not add layers.

- Acceptance criteria addressed: AC-27 (account deletion within 15 min), AC-69 (50 concurrent commits, feed gapless and ordered), AC-74 (usage equals the sum of live revisions after 1,000 concurrent commits and deletes)
- Restrictions addressed: RESTRICT-05 (Rust workspace for API, workers and daemons), RESTRICT-13 (supervisor and decoder keep no database or blob access; `media-shared` carries types only), RESTRICT-24 (an edit beats a delete, which the single commit transaction must realise)
- Risks addressed: R15 (complexity: 21 components, about 50 ports, schedule slip), R11 (quota drift, helped by one ledger writer inside the commit transaction)
- Research source: `_docs/02_document/component-decisions.md` § Known drift to fix in the pass (dependency cycles)

## Decision

We will make every cross-component call go through a port trait defined in the server-only `contracts` crate: a component depends on `contracts` and the helper crates, never on another component's crate, so the crate graph has no cycles. A dev-dependency on another component's crate is allowed (owner decision 2026-11-12): a component's tests may run against another component's real code on the real database; the crate-graph check (`server/tests/crate_graph.rs`) still rejects every normal and build edge between components. The `sunnycloud` binary (12 operator_cli) is the composition root: it builds every service, registers the late lists (`AccountLifecycleHook`, `BlobOwnerCleanup`, `Janitor`, `DoctorCheck`, `HealthContributor`, `ReadinessCheck`), constructs the scheduler and the rate limiter, and passes the assembled `ApiServices` to 11 http_api. No component depends on 12; registering lists late is what breaks the remaining wiring cycles.

The commit transaction belongs to 02 tree. `TreeCommit::publish` opens the transaction, calls `AccountGuard::ensure_active` (01) first, takes the account advisory lock, runs the `CommitEffect`s supplied by 03 and 08, applies `QuotaLedger::adjust_used` and `release_reservation` (07a) per entry in a savepoint, and calls the feed stamp (05) last before COMMIT. There is no multi-level lock-order contract. `TreeReader` absorbs `NodeResolver` so nodes have one read port. Account erase runs through one `AccountLifecycleHook { initialize, erase_batch }` implemented by 02, 03, 05, 06, 07a, 08 and 20; 07's `DeletionPurger` drives every hook until all report `remaining = 0`, and only 01 triggers hooks and finalizes the account. Delivery (11) only binds and validates, calls one business operation and maps the result through the error mapper; it holds no SQL and no business rule.

Client-side traits (`SyncApi`, `CredentialProvider`, `FileSource`, `DownloadSink`, `CheckpointStore`, `ProgressSink`) live in the client crate `sync-core`, because `contracts` pulls server dependencies. `BackupCore` is defined in `mobile_backup` and the daemon control types in `ipc_schema`. `media-shared` holds job DTOs, the transcode policy and the decoder protocol only (no sqlx, no queue module, no I/O); the queue SQL lives in 08, inside the API.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| Direct calls between component crates | Produced the cycles found at Step 4a (quota, tree, transfer, blobstore, identity, feed); 02 and 03 could not both own the quota update, which risks AC-74 drift. Ports cut the graph to 0 component-to-component edges |
| Outbox table or in-process event bus for effects (quota, feed, jobs) | Quota delta, receipt, feed counter and job enqueue must commit atomically with the revision (AC-69 needs one gapless `seq` per batch); an asynchronous bus adds a second delivery path and eventual consistency to the one place that needs none. One transaction with in-process effects is fewer parts |
| Microservices (separate identity, tree, transfer processes) | Splits the commit transaction across databases or adds sagas; one operator on one Compose host with hundreds of users (RESTRICT-28) gains nothing, and R06 already accepts one API process |
| Shared `contracts` crate for client and server traits | `contracts` pulls sqlx and server crates; a WASM or mobile build would inherit them. Client traits stay in `sync-core` |

## Consequences

### Positive

- The crate graph is acyclic by construction; a component can be built and tested against fake ports, while the black-box tier still runs real components (real PostgreSQL, no in-memory stores).
- One owner per concern: 02 for the commit transaction, 07a for the ledger, 05 for the counter, 07 for erase orchestration. Five erase traits collapse into one hook, so a new tenant-owning component adds one implementation instead of editing 07.
- The delivery layer stays thin, so the route registry generates the OpenAPI document and the AC-59 matrix without business code in handlers.

### Negative

- About 50 ports to define, document and fake; a wrong port boundary costs a change in `contracts` plus every implementer. This is the cost R15 names and the reason the vertical slice (upload, commit, feed, download) comes first.
- The composition root in 12 knows every service; a registration forgotten there is a runtime gap (an erase hook missing means data survives deletion), so the hook and cleanup lists need a startup assertion or test that all expected implementers are present.
- Effects run inside the commit transaction and must stay free of I/O, which limits what 03 and 08 can do there.

### Neutral / Open

- Not changed: auth boundary (01 `Authenticator`), the one-process v1.0 deployment (R06) and the helper crates (`sc_db`, `sc_audit`, `sc_clock`).
- The hook registration order matters and is fixed: 06 (links stop at once), 02, 03, 08, 20, then 05 and 07a, because the tree's erase batches still stamp the feed and adjust the quota (`07_admin/description.md`, `data_model.md` §12.2).

## Evidence

- `_docs/02_document/architecture.md` § Component list (Dependency rule paragraph); § 4. Data Model Overview (Data flow summary); § 5. Integration Points (Internal Communication)
- `_docs/02_document/canonical-names.md` § Ports (all in server-only `contracts`); § Runtime
- `_docs/02_document/component-decisions.md` § Known drift to fix in the pass
- `_docs/02_document/components/02_tree/description.md` § TreeCommit (owns the commit transaction); § Locking and retries; § Ports (crate `contracts`)
- `_docs/02_document/components/07_admin/description.md` § 2. Ports (declared in `contracts`)
- `_docs/02_document/components/11_http_api/description.md` § 2. Ports (declared in `contracts`); § 8. Route Registry (single source for router, OpenAPI, CSRF and IDOR generators)
- `_docs/02_document/components/12_operator_cli/description.md` § 2. Ports (declared in `contracts`); § 5. Composition root, scheduler, start-up and shutdown
- `_docs/02_document/components/13_sync_core/description.md` § 2a. Ports
- `_docs/02_document/data_model.md` § 12.2 Account deletion (AC-27, within 15 min)
- `_docs/02_document/risk_mitigations.md` § Risk Register (R15); § Defects Fixed in the Evaluator Pass (Step 4a)

## Notes

Contingency from R15: if the first vertical slice slips more than 25%, move the Linux and Windows desktop agents to v1.0.1 (ADR-016). Re-evaluate the port count after the vertical slice; merge two ports only when they always change together and have one implementer.
