# Tree security, performance and acceptance suites

**Task**: AZ-3719_tree_security_perf_suites
**Name**: Tree security and performance suites
**Description**: Run the tree's IDOR, row-level security and hostile-name tests, the listing, hot-account and batch-publish performance tests, and the stale-edit acceptance test
**Complexity**: 5 points
**Dependencies**: AZ-3707_tree_reader, AZ-3708_tree_folder_mutations, AZ-3709_tree_publish_core, AZ-3710_tree_conflict_rules, AZ-3711_tree_quota_gc_deltas, AZ-3714_tree_cascade_purge_janitors, AZ-3716_tree_revision_lookups, AZ-3718_tree_thumb_state_listing; notes: E-31/c18 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 02_tree
**Tracker**: AZ-3719
**Epic**: AZ-3640 (E-13)
**Ref**: E-13/c15

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/02_tree/tree_reader.md`, `tree_service.md`, `tree_commit.md` — the operations under test.

## Problem

The tree's guarantees that matter most to users are that another account sees nothing (AC-59), hostile names are only data (AC-37), a 10,000-entry listing answers within 300 ms (AC-6), a hot account keeps a gapless feed under load (AC-69) and a stale edit keeps both versions (AC-8). Each operation task tests its own slice; this task runs the cross-operation suites the epic lists.

## Outcome

- 0 successful cross-account calls across every `TreeService` and `TreeReader` operation and `publish`.
- Listing, hot-account and batch-publish timings measured against their targets on C-REF.
- The stale-edit acceptance scenario passes.

## Scope

### Included
- ST-02-1, ST-02-2, ST-02-3, PT-02-1, PT-02-3, the batch-publish row of PT-02-4, AT-02-1.

### Excluded
- ST-02-4 (E-13/c08); the cascade row of PT-02-4 (E-13/c10); the HTTP-level IDOR matrix (E-21/c18).

## Acceptance Criteria

**AC-1: IDOR across every operation (AC-59)**
Given account A with a rich tree and account B empty
When B calls `get_node`, `list_children`, `create_folder` (parent in A), `update_node` (A's id, and A's folder as new parent), `delete_node`, `get_revision`, `is_current_or_retained`, `resolve_live_file`, `publish` with A's node and parent, `lookup_revisions` with A's hashes and `find_revisions`
Then every call returns `NotFound`, `ParentNotFound`, `Gone` or empty, equal to the result for random ids, and a snapshot of A's rows is identical before and after

**AC-2: Row-level security with the runtime role (AC-59)**
Given role `sunnycloud` scoped to account B
When raw selects run on `nodes` and `revisions`, `ALTER TABLE nodes DISABLE ROW LEVEL SECURITY` is attempted, and `cascade_batch` runs as the janitor
Then the selects return 0 rows of A, the DDL is denied, and the janitor sees every account

**AC-3: Hostile names stay data (AC-37)**
Given the names `'; DROP TABLE nodes;--`, `%`, `_`, `a\b`, `..%2f` and a 255-byte multibyte name
When each is created, listed, resolved by path, paged with crafted cursors containing quotes, and used as a device filter with quotes
Then valid names are stored verbatim and listed once, `../x` and `a//b` resolve to `NotFound`, crafted cursors give `InvalidCursor` or a literal match, and no SQL error or extra row appears

**AC-4: Listing latency (AC-6)**
Given 8 clients paging the DS-02 folder (10 pages of 1,000) and 1 client with limit 10,000, at 20 listings per second for 5 minutes after a 30 s ramp
When latencies are measured on C-REF
Then the full listing p95 is at most 200 ms (fail 300 ms), one page p95 at most 40 ms (fail 80 ms), errors 0%, with at most 8 database connections

**AC-5: Hot account (AC-69, AC-5)**
Given 50 concurrent writers on one account making 1,000 commits over 60 s
When throughput, latency and seq values are measured on C-REF
Then throughput is at least 200 commits/s (fail 100), p95 at most 250 ms (fail 600 ms), seq gaps and duplicates 0, with at most 16 connections

**AC-6: Batch publish time (AC-40)**
Given 20 sequential 500-entry batches (10,000 files)
When each `publish` is timed on C-REF
Then p95 is at most 1.0 s (fail 2.0 s)

**AC-7: Stale edit keeps both versions (AC-8)**
Given `report.docx` at r1 on devices `laptop` and `phone`
When laptop publishes r2 on base r1, phone publishes r3 on base r1, and the folder is listed
Then `report.docx` holds r3 and exactly one `report (conflict copy YYYY-MM-DD HH-MM phone).docx` holds r2

## Non-Functional Requirements

**Performance**
- Targets as in AC-4 to AC-6; CPU at most 2 cores for PT-02-1.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| — | None (suite task) | — |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | A rich, B empty | Foreign-id calls (ST-02-1) | 0 successes; A unchanged | NFT-SEC-13, NFT-SEC-14 |
| AC-2 | Role `sunnycloud`, B scope | Raw selects, DDL, janitor (ST-02-2) | 0 foreign rows; denied; janitor sees all | NFT-SEC-46 |
| AC-3 | Hostile names | Create, list, resolve, cursors (ST-02-3) | Verbatim; `NotFound`; no SQL error | NFT-SEC-22, FT-N-41 |
| AC-4 | DS-02, 9 clients | Listing load (PT-02-1) | p95 200/40 ms | NFT-PERF-10 |
| AC-5 | 50 writers | Commits (PT-02-3) | >= 200/s; p95 250 ms; 0 gaps | NFT-PERF-08, NFT-PERF-09 |
| AC-6 | 20 x 500 entries | Batch publish (PT-02-4) | p95 1.0 s | NFT-PERF-07 |
| AC-7 | `report.docx` r1 | Two stale edits, list (AT-02-1) | One conflict copy holding r2 | FT-P-10 |

## Constraints

- Performance rows run on C-REF only (`tests/environment.md`); until REF-SRV exists they are reported `NOT-RUN(HW-REF)` (R03, E-31/c01), never measured on a workstation.
- Security rows run on the component runner (env class C) with the real runtime role.

## Risks & Mitigation

**Risk 1: No reference hardware**
- *Risk*: AC-4 to AC-6 cannot run.
- *Mitigation*: report `NOT-RUN(HW-REF)` and keep the CI plan check of E-13/c03 as the early signal.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| PT-02-3 targets are derived and must be calibrated on REF-SRV | Owner (hardware) | open | Low |
