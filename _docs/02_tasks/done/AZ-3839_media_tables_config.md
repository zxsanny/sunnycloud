# Media tables, settings and error codes

**Task**: AZ-3839_media_tables_config
**Name**: Media tables, settings and errors
**Description**: Create the `jobs`, `derived` and `worker_status` tables with their keys, indexes, grants and row policies, bind and validate the media settings, and give every 08 error one wire code
**Complexity**: 3 points
**Dependencies**: AZ-3740_migrations_setup, AZ-3782_sc_db_access, AZ-3783_sc_db_migrator_accounts_base, AZ-3723_blobstore_skeleton, AZ-3706_tree_skeleton, AZ-3763_sc_config, AZ-3764_sc_error, AZ-3652_initial_structure
**Component**: 08 media_jobs
**Tracker**: AZ-3839
**Epic**: AZ-3629 (E-18)
**Ref**: E-18/c04

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/shared_db/sc_db_migrations.md` — migration rules, roles (E-06/c01).
- `_docs/02_document/contracts/shared_db/sc_db_access.md` — account and system scope (E-06/c01).
- `_docs/02_document/contracts/shared_config/sc_config.md` — key table (E-01/c01).
- `_docs/02_document/contracts/shared_error/sc_error.md` — status table, open point 1 (E-02/c01).

## Problem

Every 08 task needs the three tables of `data_model.md` §9, the settings of 08 §8 and typed errors with wire codes. The epic row names the migration only; no other row owns the 08 settings section or the error codes that `sc_error` open point 1 leaves to each owning epic, so they are added here (3 points instead of the planned 2).

## Outcome

- One reversible migration pair in the `job` domain creates `jobs`, `derived` and `worker_status` as `data_model.md` §9 describes: the unique job key, the generated `lane` column, `jobs_claim_idx`, the revision, finished and `jobs_queued_idx` indexes, `derived` keyed by (account, content hash, kind, params version) with a unique blob id, and `worker_status` keyed by worker id.
- The API role gets DML only; `derived` is tenant-scoped; `jobs` and `worker_status` are reached in system scope.
- Settings bound and validated: `job_lease` (5 min; 5 s to 1 h), `job_attempts` (3; 1 to 10), `job_retry_backoff` (30 s base), `bulk_queue_threshold` (200), `job_terminal_ttl` (14 d), `job_reap_interval` (1 min), `derived_janitor_interval` (1 h), `derived_max_thumb` (5 MiB), `derived_max_preview` (20 MiB), `derived_max_transcode` (10 GiB), `worker_api_token` and the optional `worker_api_token_next` (secret files).
- 08 errors map to codes: `LeaseLost` to `lease_lost` (409), `RevisionObsolete` to `revision_obsolete` (404), `NotReady` to `thumbnail_not_ready` (404), derived `NotFound` to `thumbnail_unavailable` (404), worker `Forbidden` to `forbidden` (403), `OutputTooLarge` to 413, `OutputTypeRejected` and `OutputKindRejected` to 422; the 08 rows are added to the shared status table.

## Scope

### Included
- The migration, grants and row policies; the settings section; the error types and their codes.

### Excluded
- Queue behaviour (E-18/c05) and every other port (E-18/c06 to E-18/c11).

## Acceptance Criteria

**AC-1: Migration is reversible**
Given an empty database
When the migration runs up, down and up as `sunnycloud_migrator`
Then the three tables, the generated column and the indexes exist after each up and are gone after the down

**AC-2: Keys and cascade hold**
Given the tables
When a second job with the same (account, kind, content hash, params version), a second derived row with the same key and a second derived row with the same blob id are inserted, and a revision with jobs is deleted
Then the three inserts are refused, the revision's jobs are deleted with it, and `lane` is `image` for `stills` and `video` for `poster` and `transcode`

**AC-3: Grants and scope**
Given the API role `sunnycloud`
When it tries DDL on the tables, reads account A's `derived` rows in account B's scope, and reads `jobs` and `worker_status` in system scope
Then DDL is refused, B's scope sees 0 of A's rows, and system scope sees all rows

**AC-4: Indexes serve the claim and the bulk count**
Given 100,000 queued jobs over two lanes
When the claim candidate query and the per-account queued count are explained
Then the claim uses `jobs_claim_idx` and the count uses `jobs_queued_idx`

**AC-5: Settings**
Given no media keys set
When the settings load
Then the defaults are as listed; `job_lease` of 4 s or 2 h and `job_attempts` of 0 or 11 refuse start naming the key; the token files are read once, held only as secrets and never logged

**AC-6: One code per error**
Given each 08 error
When it is converted for the wire
Then it gets the code and status listed in Outcome, and each code has exactly one status in the shared table

## Non-Functional Requirements

**Performance**
- The claim candidate scan is index-only (`data_model.md` §12.5).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Defaults and bounds | As listed; out-of-range refused |
| AC-6 | Error to code conversion | As listed |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Empty database | Up, down, up | Clean each way | NFT-RES-13 |
| AC-2 | Tables | Duplicate inserts, revision delete | Refused; cascade | AC-71 |
| AC-3 | Two accounts | Role and scope checks | DDL refused; isolation | NFT-SEC-13 |
| AC-4 | 100,000 queued jobs | Plans of claim and count | Named indexes used | PT-08-1 |

## Constraints

- The migration file carries the domain word `job` (module-layout Migrations); grants in the same file.
- It sorts after the `accounts_base`, `blob` and `tree` migrations (foreign keys to accounts, blobs and revisions).

## Risks & Mitigation

**Risk 1: Queue churn bloats the table**
- *Risk*: about 20 k jobs a day leave dead tuples.
- *Mitigation*: low autovacuum scale factor and low fillfactor on `jobs` (`data_model.md` §9.1).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Code names for the 413 and 422 output refusals are not given (08 §5, sc_error open point 1) | E-18 / E-02 owners | open | Medium |
| `data_model.md` §12.3 lists `jobs` among the tables with the tenant row policy; 08 §6 says `jobs` has no tenant policy. Either way the claim runs in system scope | E-18 owner | open | Low |
| The autovacuum scale factor and fillfactor values for `jobs` are not given | E-18 owner | open | Low |
| The claim in `data_model.md` §9.1 is written with a literal `attempts < 3`, while `job_attempts` is configurable (1 to 10); the setting is used | E-18 owner | open | Low |
