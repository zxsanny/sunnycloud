# Test Specification — 12 Operator CLI

Scope: the `sunnycloud` binary: the command surface of `canonical-names.md` ("Operator commands"), exit codes (0 success, 1 warnings only, 2 failed/refused/findings, 3 usage or confirmation error), `--json` output `{result, code, data}`, the composition root, the `serve` start and stop sequence, the scheduler, `doctor`, and `migrate`. 12 ports: `Janitor` registry (`Scheduler`), `ReadinessCheck` (`scheduler_alive`), `DoctorCheck` registry. Tests run commands as the operator does (`docker compose exec|run api sunnycloud ...`) against the Compose test stack with real PostgreSQL, blob volume and master key. Harness superuser access (database, blob directory) is used only to arrange faults and to assert; the SUT never uses it. System scenarios are cited by ID, not restated.

## Acceptance Criteria Traceability

| AC ID | Acceptance Criterion | Test IDs | Coverage |
|-------|---------------------|----------|----------|
| AC-22 | `doctor --pre-flight` redirect URIs; `doctor` exit 0 as install check | IT-12-15, AT-12-1; sys FT-P-69, NFT-PERF-30 | Covered by this file and system scenarios (the 30 minute walk is MAN) |
| AC-23 | Create/reset admins and setup token without the web | IT-12-2, IT-12-3; sys FT-P-64, FT-P-65, FT-N-20, FT-N-21 | Covered by this file and system scenarios |
| AC-24 | `scrub run/status` | IT-12-14; sys FT-P-43, NFT-SEC-25, NFT-SEC-26 | Covered by this file and system scenarios |
| AC-27 | `admin user delete` drives the normal deletion path | IT-12-3, IT-12-7; sys FT-P-19, NFT-SEC-27 | Covered by this file and system scenarios |
| AC-30 | `admin restore` by path and time; `admin gc-run` | IT-12-4, IT-12-16; sys FT-P-16, FT-P-17, FT-P-18, FT-N-35 | Covered by this file and system scenarios |
| AC-41 | `admin epoch-bump`; `.epoch` marker check in doctor and `serve` | IT-12-5, IT-12-6, AT-12-2; sys NFT-RES-07, FT-P-13, FT-P-14 | Covered by this file and system scenarios |
| AC-43 | `admin rewrap`, `admin keycheck-init` (sample unwrap first), `key-check` | IT-12-12, IT-12-13; sys FT-P-41, FT-N-33, FT-P-71 | Covered by this file and system scenarios |
| AC-65 | `migrate`, restore drill, `scrub run --full` exit code | IT-12-10, IT-12-11, AT-12-2; sys FT-P-71, FT-P-72, FT-N-39, NFT-RES-13, NFT-RES-14 | Covered by this file and system scenarios (failing, killed and irreversible migrations through the test-only migration source, IT-12-11) |
| AC-72 | `key-check` and `doctor` fail on a wrong master key; key check precedes listeners | IT-12-6, IT-12-12, IT-12-15; sys FT-N-43 | Covered by this file and sys FT-N-43 |
| AC-73 | `doctor` lists every revision with a missing blob; listings stay error-free | IT-12-15; sys NFT-RES-37 | Covered by this file and sys NFT-RES-37 |
| AC-74 | Doctor's quota recompute reports drift | IT-12-15; sys NFT-RES-38 | Covered by this file and sys NFT-RES-38 |
| AC-80 | `scrub run` is safe beside a live API | IT-12-14, IT-12-9; sys NFT-RES-39 | Covered by this file and sys NFT-RES-39 |
| AC-68 | Scheduler cycles are the only janitors (orphan sweep, session expiry) | IT-12-9; sys NFT-RES-34 | Covered by this file (scheduler) and sys NFT-RES-34 |
| AC-64 | `openapi` output used by the CI diff | IT-12-18; sys FT-P-70 | Covered by this file and sys FT-P-70 |

AC-75, AC-76 (invite and device-code atomicity) belong to 01; the CLI only issues invites (checked in IT-12-3). No AC is without a test; failing-migration, irreversible and interrupted-cancel cases are BLOCKED or ambiguous and listed under Spec gaps.

## Blackbox Tests

Harness `H2` (Compose `core` stack, test image): services `db`, `migrate`, `api`, `worker-supervisor` as in `environment.md`; commands run through `e2e-ctl exec-operator` with exit code, stdout and stderr captured separately. Test config: scheduler intervals 200 ms, `SUNNYCLOUD_SHUTDOWN_GRACE=3s`, retention 7 d with the test clock for day jumps, `cli_progress_interval=500ms`. A "restore drill" means the DS-22 flow (`pg_dump`, wipe, `pg_restore`).

### IT-12-1: Exit codes, usage and `--json`
**Summary**: the exit-code table and JSON shape hold for every command class.
**Traces to**: AC-23, AC-65
**Input data**: unknown command; missing required argument; `admin user delete <id>` without `--yes` on a non-TTY; with `--yes`; `admin rewrap` and `migrate revert` without `--yes`; `doctor` on a healthy stack; `doctor` with one warning; `doctor` with one failure; `--json` on each.
**Expected result**: usage errors and missing confirmation -> exit 3 and no state change; healthy doctor 0, warnings only 1, any failure 2; refused-by-owner results 2; `--json` prints exactly one object `{result, code, data}` with `code` equal to the owner's stable error code (for example `last_admin`) and the same facts as text mode; text goes to stdout, diagnostics to stderr.
**Max execution time**: 60 s
**Dependencies**: stack

### IT-12-2: `admin create`, `admin setup-token`, `admin reset`
**Summary**: bootstrap commands print secrets once and refuse out-of-state use.
**Traces to**: AC-23
**Input data**: empty database (`golden+empty`); `admin setup-token` twice; first account created via the token (consumer, mock-idp); `admin setup-token` again; `admin create --note x --ttl 1h`; `admin reset <admin>`; `admin reset <unknown>`; `admin reset <last admin>` while it is the only admin.
**Expected result**: setup-token prints a token and a hint, the second call replaces the open token (first no longer works); after any account exists: exit 2 `SetupNotAvailable`; `admin create` prints an invite token once (exit 0), only its SHA-256 is in the database (harness search for the token as bytes/text: 0 hits), token length >= 32 random bytes encoded; `--ttl 1h` expires after the test clock moves 61 min; `reset` revokes the target admin's sessions (old session 401) and prints a fresh invite once; unknown account exit 2 `NotFound`; reset of a sole active admin succeeds like any other (exit 0, fresh invite once, owner decision G-12-3, 2026-11-11); no token appears in logs, `--json` log lines or audit rows.
**Max execution time**: 60 s
**Dependencies**: stack, mock-idp

### IT-12-3: `promote`, `demote`, `user`, `invite`
**Summary**: role, quota, deletion and invite commands call the owners and honor the last-admin rule.
**Traces to**: AC-23, AC-27
**Input data**: two admins A, B and user U; `admin demote A` and `admin demote B` run at the same moment from two processes; `admin promote U`; `admin user list`; `admin user set-quota U 1GiB`; `admin user delete U --yes`; `admin user delete <last admin> --yes`; `admin invite create`, `list`, `revoke <id>`.
**Expected result**: concurrent demotes -> exit codes {0, 2}, one admin remains active; sole-admin demote/delete exit 2 with `last_admin`; `set-quota` visible in `GET /accounts/me/quota`; `user list` shows role, state, used, limit and no tokens; `user delete U --yes` exit 0, U `deleting`, then 0 rows after the purger (<= 30 s test deadline); invite token printed once on `create`, never by `list`; after `revoke` a redemption fails (consumer); one `admin_action` row per mutating command with actor `Operator`, ids only.
**Max execution time**: 120 s
**Dependencies**: stack

### IT-12-4: `admin restore`
**Summary**: restore re-points the existing revision row and reports placement.
**Traces to**: AC-30
**Input data**: DS-23 file overwritten twice, then deleted; retention 7 d; cases: original folder live (`Original`), original folder deleted (`Restored`), `--dry-run`, `--at` beyond retention (test clock +8 d), unknown path, a path matching a folder.
**Expected result**: `Restored{revision, placement}` printed; `Original`: file back at its path with the original content hash and a feed change visible to a consumer device; `Restored`: file under a root folder "Restored", original hash; `revisions` row count unchanged and `revisions.blob_id` still unique (no second revision for the blob); the `--dry-run` writes nothing (row counts equal); beyond retention exit 2 `BeyondRetention`; unknown exit 2 `NotFound`; the restored file is downloadable by the user afterwards. The accepted format of `--at` is not given in the description (gap G-12-2); the test uses RFC 3339 UTC.
**Max execution time**: 90 s
**Dependencies**: stack, test clock

### IT-12-5: `admin epoch-bump`
**Summary**: the bump raises the database epoch, writes the `.epoch` marker and prints reconcile findings.
**Traces to**: AC-41
**Input data**: DS-22 restore drill state (older DB, newer blobs); `admin epoch-bump` without `--yes` (non-TTY) and with `--yes`; a second bump; an attempt to set the marker lower by hand then run `doctor`.
**Expected result**: without `--yes` exit 3 and no change; with `--yes` the new epoch is printed, `.epoch` on the blob volume (read by the harness) equals it, the `restore`-tagged doctor checks run and list rows without blobs and orphans; exit 0 (or 1 with findings, see gap G-12-5); the second bump produces a strictly higher epoch and marker; connected devices receive `resync_required` after restart (sys NFT-RES-07); a lowered marker never causes a lower epoch to be accepted (marker "only ever rises": doctor reads the larger of the file and the database as the guard).
**Max execution time**: 60 s
**Dependencies**: stack, DS-22

### IT-12-6: `serve` start refusals
**Summary**: `serve` refuses to start, before opening any listener, on each unsafe condition.
**Traces to**: AC-41, AC-65, AC-72
**Input data**: start the api container with: (a) a pending migration (database one version behind); (b) database newer than the binary; (c) `.epoch` marker above the database epoch; (d) wrong master key; (e) master key file missing; (f) `.keycheck` absent but blobs exist; (g) unknown `SUNNYCLOUD_BOGUS=1`; (h) retention 91; (i) no sign-in provider configured; (j) `db_api_url` that points at the schema-owner role.
**Expected result**: each exits non-zero (2 or 3) within 10 s; ports 8080, 8081 and 9090 never accept a connection (probe every 100 ms from start); message names the fix (`sunnycloud migrate` for a/b, `admin epoch-bump` for c, key file for d to f, the unknown key for g, the allowed range for h, providers for i, the role check for j); no key bytes in output; `healthcheck` returns non-zero; control run with everything valid starts.
**Max execution time**: 120 s (10 container starts)
**Dependencies**: stack, ctl

### IT-12-7: `serve` start sequence and late registration
**Summary**: start order is config, role, schema, key check, epoch marker, services, listeners, late lists, scheduler; every hook and janitor is registered.
**Traces to**: AC-27, AC-68, AC-72
**Input data**: start with `SUNNYCLOUD_LOG=json`; then create an account with data in every owner (nodes, revisions and shares, an upload session with staged rows and reservation, feed rows, a photo with jobs and derived rows, a quota row, [v1.2 vault rows skipped]); `admin user delete --yes`; run the janitor list check through `/metrics` (`janitor_cycles_total{name}`).
**Expected result**: log events appear in the documented order with increasing timestamps; listeners open only after the key and epoch checks; `/readyz` includes `scheduler_alive`; after deletion the table sweep (every table with `account_id`, plus identity tables) shows 0 rows within the deadline, so 06, 02, 03, 08, 05, 07a hooks were all registered; derived rows of a GC'd blob disappear (`BlobOwnerCleanup` for 08 registered); janitors present: 03 (session reaper, receipt janitor, staged expiry), 04 (GC sweeper, orphan sweep, scrub), 02 (tombstone purge, folder cascade), 07 (purger), 01 (grants, oauth states, sessions), 08 (`JobReaper`, `DerivedJanitor`), helper 04 (audit retention): each name has a nonzero cycle counter within 10 s at 200 ms intervals.
**Max execution time**: 90 s
**Dependencies**: stack

### IT-12-8: Graceful shutdown
**Summary**: SIGTERM runs the documented shutdown order within the grace period.
**Traces to**: AC-65
**Input data**: stack with 20 held long-polls and 10 uploads; `docker stop -t 10 api` (SIGTERM); `SHUTDOWN_GRACE=3s`.
**Expected result**: log order: readiness 503, listeners stop accepting, long-polls woken, connections drained, scheduler stopped, upload progress checkpoints flushed, final group sync, pool closed; process exit code 0 within grace + 2 s; a restart finds no unrecoverable state (unacked blocks resent by clients; sys NFT-RES-22).
**Max execution time**: 40 s
**Dependencies**: stack

### IT-12-9: Scheduler behaviour
**Summary**: failing cycles are survived, cycles do not overlap, CLI and scheduler share claim-safe work.
**Traces to**: AC-80, AC-68
**Input data**: in-process `Scheduler` test binary with three `Janitor`s: one always erroring, one panicking once, one counting `run_cycle` calls (intervals 200 ms); on the stack: `admin gc-run` executed 20 times while the scheduler's GC sweeper and `scrub run --full` run, with 50 blobs in `blob_gc` due.
**Expected result**: erroring janitor logged at ERROR with its name and a failure metric each cycle, loop continues; panic is caught and logged, loop continues; the counting janitor runs at 200 ms +/- 100 ms; no janitor has two overlapping `run_cycle` calls; no blob is deleted twice or reported corrupt (scrub exit 0); `gc-run` prints counts and exit 0 whether or not the scheduler took rows (claims via `SKIP LOCKED`); the scheduler uses no advisory lock (`pg_locks` shows none of type advisory held across a cycle).
**Max execution time**: 60 s
**Dependencies**: stack, in-process harness

### IT-12-10: `migrate`
**Summary**: the migration step uses the migrator role and is serialized and checked.
**Traces to**: AC-65
**Input data**: fresh database; `migrate` twice; two `migrate` processes at once; `migrate status`; `_sqlx_migrations` row edited by the harness (checksum changed); a row with a future version; a row marked unsuccessful; `migrate revert --steps 1` without and with `--yes`; `migrate` with the master key file missing and with the API container's environment.
**Expected result**: first run applies all and exits 0; second is a no-op, exit 0; concurrent runs: each migration version exactly once, both exit 0 or the loser waits, no interleaving; `status` lists applied/pending; checksum mismatch -> exit 2 naming the version; future version -> `status` exit 2 `unknown-newer`; unsuccessful row reported `dirty`; `revert` without `--yes` exit 3, with `--yes` reverts one migration in its own transaction; `migrate` needs no master key; the API container environment has no `db_migrator_url` (`ctl inspect`), and `migrate` run there exits 3 (configuration error).
**Max execution time**: 120 s
**Dependencies**: stack, PostgreSQL

### IT-12-11: Failing, killed and irreversible migrations
**Summary**: a failing migration rolls back, a killed one leaves the previous state, an irreversible `down` stops `revert`; the fixtures come through the test-only migration source of description section 4 (feature `test-migrations`, `SUNNYCLOUD_TEST_MIGRATIONS_DIR`).
**Traces to**: AC-65
**Input data**: test image (`FEATURES=test-clock,test-migrations`); fixtures directory mounted read-only at `/test-migrations` with three subdirectories; each case sets `SUNNYCLOUD_TEST_MIGRATIONS_DIR` to one subdirectory on both the `migrate` step and `serve`, and starts from the restored golden snapshot (release migrations applied). Versions use the `99990101......` prefix.
- `fail/`: `..01_test_ok` (reversible, `CREATE TABLE test_ok`), `..02_test_fail` (`CREATE TABLE test_fail_marker (id int); SELECT 1/0;`).
- `slow/`: `..01_test_slow` (`CREATE TABLE test_slow_marker (id int); SELECT pg_sleep(120);`).
- `irreversible/`: `..01_test_irreversible` (up creates `test_irrev`; `down` is `DO $$ BEGIN RAISE EXCEPTION 'irreversible: restore from backup'; END $$;`), `..02_test_reversible` (up creates `test_rev`, down drops it).
- Release image: `sunnycloud migrate status` with `SUNNYCLOUD_TEST_MIGRATIONS_DIR=/tmp` and no database URL (the CI step of description section 4, on the pushed digest).
**Expected result**:
- `fail/`: `migrate` exits 2 naming version `..02`; `test_ok` exists (`..01` stayed applied), `test_fail_marker` does not (rolled back); `migrate status` lists `..02` pending; a second `migrate` retries `..02` and exits 2 again.
- `slow/`: the harness `kill -9`s the `migrate` process once `pg_sleep` is visible in `pg_stat_activity`, then ends the orphaned backend with `pg_terminate_backend` (harness superuser); `test_slow_marker` does not exist, `migrate status` lists `..01` pending, and `serve` (same variable) refuses to start with the message naming `sunnycloud migrate`, serving 0 requests (the interrupted-upgrade behaviour of sys NFT-RES-14, which uses real release migrations).
- `irreversible/`: `migrate` exits 0; `migrate revert --steps 2 --yes` reverts `..02` (`test_rev` gone), then `..01`'s `down` raises, the output shows `IrreversibleMigration` with the message `irreversible: restore from backup`, exit 2; `test_irrev` still exists and `..01` is still applied.
- Release image: exit 3, the message names the unknown key `SUNNYCLOUD_TEST_MIGRATIONS_DIR`, no database connection attempted; the binary contains no `test_migrations_dir` string.
- After each case: golden snapshot restored (no database that applied a fixture is reused).
**Max execution time**: 120 s
**Dependencies**: stack, PostgreSQL, test image built with `test-migrations`, fixtures mount (E-00), E-06/c01b and E-01 pieces listed in description section 4

### IT-12-12: `key-check`, `keycheck-init`
**Summary**: key verification outcomes and the sample-unwrap guard.
**Traces to**: AC-43, AC-72
**Input data**: correct key; missing file; 31-byte file; wrong key; `.keycheck` deleted with blobs present; `admin keycheck-init` with the wrong candidate on a volume with 100 blobs; with the correct key; on an empty volume.
**Expected result**: correct -> exit 0, no key bytes in stdout, stderr, `--json` output or logs (search for hex and base64 of the key); `FileMissing`, `WrongLength`, `KeyCheckMismatch`, `KeyCheckAbsentButBlobsExist` each exit 2 with that code in `--json`; `keycheck-init` with the wrong key exit 2 `KeyCheckSampleFailed` and `.keycheck` not created; correct key writes it; empty volume writes it without samples.
**Max execution time**: 60 s
**Dependencies**: stack, 04

### IT-12-13: `admin rewrap`
**Summary**: key rotation rewrites wrapped keys, not blobs.
**Traces to**: AC-43
**Input data**: 500 blobs (DS-09 slice plus small files); `admin rewrap --new-key-file k2 --yes`; then restart with k2; also a run interrupted after 40% (kill the CLI process) and rerun; and without `--yes`.
**Expected result**: loop runs until 0 rows remain, progress lines on stderr every `cli_progress_interval`, a message that the old key must be kept; blob files unchanged byte for byte (SHA-256 of every file before/after equal); after restart with k2 all downloads hash-equal; with the old key `key-check` fails; the interrupted run is resumable (second run finishes, no double rewrap error); no `--yes` -> exit 3 and nothing changed.
**Max execution time**: 120 s
**Dependencies**: stack, 04

### IT-12-14: `scrub run` and `scrub status`
**Summary**: scrub exit codes, modes, throttle and safety next to live traffic.
**Traces to**: AC-24, AC-80
**Input data**: clean volume of 100 blobs; one bit flipped in a blob by ctl; `scrub run` (Due) and `scrub run --full`; `--rate 5` on a 20 MiB blob; `scrub status`; then `scrub run --full` while 100 uploads are in progress and GC is deleting 50 blobs.
**Expected result**: clean run exit 0; flipped bit -> exit 2 with the blob id and path in findings; `--full` visits all blobs, default only due ones (counts from `status`); `--rate 5` takes between 3.5 s and 6 s for 20 MiB; `status` prints oldest `last_scrubbed_at`, unscrubbed count, open `scrub_mismatch` findings and suspect set, exit 0; during uploads and GC: exit 0, 0 findings (no `uploading`/`removed` blob reported corrupt); summary also in the audit log and metrics.
**Max execution time**: 120 s
**Dependencies**: stack, ctl

### IT-12-15: `doctor`
**Summary**: aggregation of owners' checks, levels, read-only guarantee, no secrets.
**Traces to**: AC-22, AC-72, AC-73, AC-74, AC-41
**Input data**: healthy stack; then each fault in turn: blob file deleted for 3 revisions (DS-22 restore drill state); `used_bytes` +1000 on one account; `.epoch` marker above the database epoch; no `worker_status` row; `image_age_days = 15`; master key swapped; `db_api_url` role with `BYPASSRLS`; retention 1 d with `--backup-interval 7d`; plus `--pre-flight` before the stack starts (no database).
**Expected result**: healthy: exit 0; missing blobs: the 3 revisions listed with path, account and revision id, status Fail (exit 2; the description says "2 or 1 by policy", gap G-12-6), while `GET` listing of their folders returns 200 and the files show as unavailable; drift: Warn or Fail with `Drift{stored, computed}`; marker above epoch: Fail naming `admin epoch-bump`; no worker: Warn (Fail when jobs are queued); image age: Warn; wrong key: Fail; role attributes: Fail; retention shorter than backup interval: Warn; `--json` carries identical findings; doctor performs no writes (`pg_stat_user_tables` insert/update/delete deltas 0 apart from audit if it records the run) and prints no secret, token, key byte or database password; `--pre-flight` prints the exact redirect URIs per configured provider and checks config, key length and volume without a database.
**Max execution time**: 180 s
**Dependencies**: stack, 01, 02, 04, 05, 07a, 08

### IT-12-16: `gc-run`, `media-requeue`, `media-stats`
**Summary**: operator maintenance commands delegate to their owners.
**Traces to**: AC-30
**Input data**: file overwritten, test clock at day 6 and day 8; `admin gc-run`; 4 dead jobs for 2 accounts and 2 kinds; `admin media-requeue --account A`, `--kind transcode`, no filter; `admin media-stats`.
**Expected result**: day 6: count 0 removed, blob present; day 8: blob removed, count 1, drained until nothing due; requeue prints counts that match the filters (and `queued` rows after); `media-stats` prints depth per lane and status equal to a harness query; exit 0 each; none uses SQL in the CLI crate (grep test in CI: no `sqlx`, no blob path strings).
**Max execution time**: 60 s
**Dependencies**: stack, test clock

### IT-12-17: `audit export`, `dev-seed`
**Summary**: audit lines carry ids only; dev-seed refuses production.
**Traces to**: AC-23
**Input data**: run the mutating commands of IT-12-2/3/4/13; `admin audit export --since <T>` before and after; `environment=production` and `development` for `admin dev-seed`.
**Expected result**: JSON lines, one per event after `T`, with ids and action codes; none contains a name, email, token or path; every state-changing command left one `admin_action`-class event with actor `Operator` and the command name; `--since` filter exact; `dev-seed` in production exits 2 and writes nothing; in development exits 0.
**Max execution time**: 60 s
**Dependencies**: stack

### IT-12-18: `openapi`, `healthcheck`, `version`, lazy construction
**Summary**: local commands need only what they use.
**Traces to**: AC-64, AC-22
**Input data**: `openapi` twice with no database and no master key; compare to `GET /api/v1/openapi.json`; `healthcheck` on a ready stack and with the database stopped; `version`; `migrate`, `admin setup-token` and `key-check` with unrelated config missing (no provider, no worker token).
**Expected result**: `openapi` output is valid JSON, byte-identical on both runs and equal to the served document; `healthcheck` 0 when `127.0.0.1:9090/readyz` is 200, non-zero on 503; `version` prints a semver and the git commit; the three commands in the last case succeed.
**Max execution time**: 40 s
**Dependencies**: stack

### IT-12-19: Secret output channel and cancellation
**Summary**: secrets go once to stdout; Ctrl-C cancels cooperatively.
**Traces to**: AC-23, AC-24
**Input data**: `admin create` with stdout and stderr redirected separately and with `SUNNYCLOUD_LOG=json`; SIGINT sent to `scrub run --full` and `admin rewrap --yes` at 30%.
**Expected result**: the token is only on stdout, once; stderr and JSON logs hold none; SIGINT: the process stops within 5 s without corrupting state (scrub run closed with `finish_run` or marked incomplete; rewrap resumable); exit code on cancellation is not specified (gap G-12-7), recorded.
**Max execution time**: 60 s
**Dependencies**: stack

## Performance Tests

### PT-12-1: Command latencies at scale
**Summary**: operator commands stay usable with 500 accounts and 100,000 revisions.
**Traces to**: AC-22
**Load scenario**:
- DS-29 plus DS-11 tree (100,000 files) loaded; commands run serially: `admin user list`, `doctor`, `doctor --json`, `scrub status`, `admin media-stats`, `serve` to ready

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| `admin user list` (500) | 1 s | 3 s |
| `doctor` full | 60 s | 180 s |
| `scrub status` | 2 s | 10 s |
| `serve` start to `/readyz` 200 | 10 s | 30 s |

**Resource limits**:
- API container memory under 1 GiB during `doctor`. Targets are proposed (the description gives none); `doctor` on 100,000 revisions exercises `BlobInventory::find_missing` and `describe_blobs`.

### PT-12-2: Scheduler overhead
**Summary**: idle janitor cycles are cheap.
**Traces to**: AC-68
**Load scenario**:
- 20 active accounts, scheduler at production intervals for 10 minutes, no traffic

**Expected results**:

| Metric | Target | Failure Threshold |
|--------|--------|-------------------|
| CPU of api container | 2% | 5% |
| Queries per janitor cycle with nothing due | 3 or fewer | 10 |
| Cycle duration p95 | 20 ms | 100 ms |

**Resource limits**:
- Database connections used by the scheduler: at most 2.

## Security Tests

### ST-12-1: Secrets never leave the secret channel
**Summary**: tokens and keys appear once on stdout and nowhere else.
**Traces to**: AC-23, AC-43
**Attack vector**: log scraping, CI output capture, process arguments.
**Test procedure**: run IT-12-2, IT-12-12, IT-12-13 with a known master key and tokens; grep stdout of non-secret commands, all stderr, container logs (text and JSON), audit rows, `doctor --json`, `ps` output of the host for argument leakage (`--new-key-file` takes a path, not the key).
**Expected behavior**: each token appears exactly once (the creating command's stdout); keys never appear.
**Pass criteria**: counts as stated.
**Fail criteria**: any extra occurrence.

### ST-12-2: Destructive commands need confirmation
**Summary**: no destructive command runs on a non-TTY without `--yes`.
**Traces to**: AC-23, AC-43
**Attack vector**: scripted or piped invocation by mistake.
**Test procedure**: run `admin user delete`, `admin rewrap`, `admin epoch-bump`, `migrate revert` with stdin from `/dev/null` and no `--yes`.
**Expected behavior**: exit 3 and no change; with a TTY the confirmation names the target.
**Pass criteria**: 0 state changes.
**Fail criteria**: any.

### ST-12-3: No listener and no SQL of its own
**Summary**: the CLI opens no network port and the crate contains no SQL or blob paths.
**Traces to**: AC-23
**Attack vector**: unintended exposure; business logic in the delivery layer.
**Test procedure**: `ss -ltnp` snapshots during `doctor`, `scrub run`; CI grep of the crate for `sqlx`, `blobs/`; system-scope DB access only through the single repository module (grep).
**Expected behavior**: no new listening socket from CLI processes; grep finds nothing.
**Pass criteria**: both clean.
**Fail criteria**: any hit.

### ST-12-4: Database role boundaries
**Summary**: the runtime role cannot migrate and the migrator URL is absent from the API container.
**Traces to**: AC-65
**Attack vector**: compromised API process changing the schema.
**Test procedure**: inside the api container try `DROP TABLE` through the configured connection string; read its environment and mounted secrets for `db_migrator_url`.
**Expected behavior**: permission denied; no migrator secret present.
**Pass criteria**: both hold (sys NFT-SEC-46 repeats this system-wide).
**Fail criteria**: any DDL success or secret found.

## Acceptance Tests

### AT-12-1: Install check
**Summary**: an operator's pre-flight and doctor give a clean verdict.
**Traces to**: AC-22
**Preconditions**:
- Fresh Compose stack from the release files, one provider configured.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | `sunnycloud doctor --pre-flight` before start | prints exact redirect URIs; config, key length, volume ok |
| 2 | `docker compose up -d`; `sunnycloud admin setup-token`; first sign-in | account created |
| 3 | `sunnycloud doctor` | exit 0 |

### AT-12-2: Restore drill
**Summary**: dump, wipe, restore, bump, scrub, doctor passes; skipping the bump fails.
**Traces to**: AC-41, AC-65
**Preconditions**:
- DS-22 backup set on a clean stack.

**Steps**:

| Step | Action | Expected Result |
|------|--------|-----------------|
| 1 | Restore DB, blobs and key; start `serve` without a bump | refuses to start naming `admin epoch-bump`; `doctor` Fail on the marker |
| 2 | `admin epoch-bump --yes` | new epoch, findings printed |
| 3 | `scrub run --full` | exit 0 |
| 4 | `doctor` | exit 0 (or warnings only) |
| 5 | `serve` | ready; devices receive `resync_required` (sys NFT-RES-07) |

## Test Data Management

**Required test data**:

| Data Set | Description | Source | Size |
|----------|-------------|--------|------|
| DS-22 | backup set of release N-1 | `gen_backup_set.py` | about 2-3 GB |
| DS-29 | 500 accounts | ctl | about 50 MB DB |
| DS-23 | small fixtures incl. overwrite and delete files | `gen_tree.py` | small |
| DS-09 slice, DS-34 | blobs for rewrap/scrub and concurrent uploads | generators | about 2.5 GiB |
| DS-33 | per-run master key, second key, setup token | generated at stack start | tiny |
| DS-11 | 100,000-file tree for PT-12-1 | `gen_tree.py` | about 0.5 GB |

**Setup procedure**:
1. Start the `core` stack by digest; the `migrate` step runs once; bootstrap with the setup token; take the golden snapshot (pgdata tar and XFS image reflink).
2. Per test: `tests/run.sh reset` (under 60 s) for group X tests; each CLI test creates its own accounts via `admin create` and invites.

**Teardown procedure**:
1. Reset the clock offset to 0; remove marker or key edits made by the test; restore the golden snapshot; leak check of blob bytes and row counts.

**Data isolation strategy**: destructive and clock tests are exclusive (group X) and start from a restored stack; read-only commands run in group P with their own accounts. Real PostgreSQL, real XFS volume and the real binary are used throughout; the in-process scheduler harness (IT-12-9) uses test `Janitor` implementations only to inject failures into the real `Scheduler`.

## Spec gaps found

- G-12-1 (resolved 2026-11-11, AZ-3787, agent decision, owner may overturn): feature `test-migrations` of the test image reads `SUNNYCLOUD_TEST_MIGRATIONS_DIR`; a release build refuses the variable (exit 3); see description section 4 and IT-12-11.
- G-12-2: `admin restore --at` and `--backup-interval` formats are not stated.
- G-12-3 (resolved 2026-11-11, AZ-3681): reset of the sole admin is allowed; no `LastAdmin` applies.
- G-12-5: `admin epoch-bump` exit "0 or 1" conflicts with the table saying 1 means warnings only for doctor; what produces 1 is not defined.
- G-12-6: `doctor` exit code for missing blobs ("2 or 1 by policy") has no stated policy.
- G-12-7: exit code on SIGINT cancellation is not defined.
