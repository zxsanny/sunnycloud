# Batch Report

**Batch**: 69
**Tasks**: AZ-3704_sharing_acceptance_ref_timing (done; REF timing `NOT-RUN(HW-REF)`), AZ-3823_admin_security_acceptance (done), AZ-3825_admin_hook_chain_integration (done)
**Date**: 2026-11-14

Two parallel workers: 06 (AZ-3704) and 07 (AZ-3823 then AZ-3825). All three are test tasks; no production defect was found (the only `src` change is a `#[cfg(test)]` unit module in 07 `user_admin.rs` that AZ-3823's unit row asks for). The parent wrote one leftover for every reference-server timing row still not run (`_docs/_process_leftovers/2026-11-14_ref-srv-timing-not-run.md`), recorded the owner's composition-root decisions in AZ-3789, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3704_sharing_acceptance_ref_timing | Done (timing not run) | AT-06-1..3 pass at the port boundary (shared photo opens, downloads once, `NotFound` after revoke; password link limits wrong guesses and grants after the window; a link follows edits, renames and moves and is `NotFound` on the next call after delete). PT-06-1..3 timing halves stay ignored; they now print the host line and a per-metric verdict for the REF-SRV run |
| AZ-3823_admin_security_acceptance | Done | ST-07-1 (user session and admin device token `Forbidden` on every admin port, unknown and nil ids one identical `NotFound`, nothing written), ST-07-4 (no name or email in audit JSON, logs or metrics; admin-action detail is exactly `{"action": …}`), AT-07-1 port form (purger dropped mid-step, fresh one finishes: 0 rows, 0 files, 0 links within 60 s), AT-07-2 port form (the only active admin survives every path) |
| AZ-3825_admin_hook_chain_integration | Done | IT-07-9/10 with the real 08 `MediaLifecycle` in the v1.0 order 06, 02, 03, 08, 05, 07a: derived blobs queued `account_deleted`, no orphan blob after any cycle, every table at 0; a failure at the media hook is logged (`hook position 3`, 0-based per contract) and the purge resumes |

## Code Review Verdict: PASS

CI-parity (`CARGO_INCREMENTAL=0`): `cargo fmt --all --check`, workspace clippy `--all-targets --locked -D warnings`, `check-no-sql-now`, `check-db-gates` (3 of 3), `check-audit-sql`, `build-wasm` exit 0; `cargo test --workspace --locked --no-fail-fast` on PostgreSQL 18: 3692 passed, 0 failed, 31 ignored. `rust-lint.sh` steps 3-4 NOT-RUN(env: no `psql`); no migration in this batch.

Notes (recorded):
- 08's jobs are removed by 02's revision erase (FK cascade) before 08 runs; 08 erases derived rows in the chain.
- AZ-3825 AC-2 says "position 4"; the purger logs the 0-based index per contract (`hook position 3`).
- The vault hook (20) is not in v1.0 builds (owner decision for AZ-3789: `vault` feature off by default).

## Owner decisions recorded this batch (for AZ-3789)

2026-11-14: composition root with plain lazy builders (no registry); vault crates behind a `vault` feature of the server crate, off by default; 08's media enqueue effect takes the account from each commit (one shared instance; also unblocks AZ-3822's media part).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | DS-03 photo not available at this tier | AZ-3704 | DS-03-shaped revision (3 MiB `image/jpeg`) | clear |
| 2 | No lookup cache: "within the window" checked as the next call | AZ-3704 | Stricter than the spec | clear |
| 3 | REF-SRV model and availability | AZ-3704 flag (Medium) | Open, owner; leftover written | unclear |
| 4 | Audit-write-failure metric needs `sc_observability`'s `db` feature, not enabled in 07 | AZ-3823 | Noted in the spec | clear |
| 5 | AT-07-1/2 port form here, stack form in AZ-3665 / FT-N-46 | AZ-3823 flag (Low) | Kept as recorded | clear |

## Commit

`[AZ-3704] [AZ-3823] [AZ-3825] Sharing acceptance, admin security, hook chain` — Loop: 1

## Next Batch

AZ-3789 composition root (alone: it touches the server crate and wires every component).
