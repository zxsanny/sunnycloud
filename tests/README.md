# Black-box test harness (skeleton, AZ-3742)

Design: `_docs/02_document/tests/environment.md`. This directory holds the shared entry command, the test compose file, the
Python consumer with its CSV reporter, and the `e2e-ctl` stub. Pipeline tasks and the E-31 scenarios plug into it.

## Run

    tests/run.sh --profile core            # start db + consumer, run the self-tests, tear everything down
    tests/run.sh --profile core --keep     # same, but leave the stack up
    tests/run.sh reset                     # restore the database volume to the golden snapshot (stack must be up)
    tests/run.sh down                      # tear down a kept stack
    SEED_FAIL=1 tests/run.sh --profile core   # adds one failing self-test; the run must exit non-zero
    tests/tools/check-reset.sh             # needs --keep: writes rows, resets, checks they are gone and the time

Needs on the host: Docker with Compose, `openssl`, `python3`. The `core` run takes about 30 s.

Profiles: `core`, `web`, `desktop`, `android-emu`, `perf`, `lan`, `gpu`. Only `core` has services yet; the others exit 3 with
"not available yet" (never a pass). An unknown name prints the usage and exits 2.

Exit codes: 0 all good, 1 a FAIL or a setup error, 2 usage, 3 profile not available yet.

## What `core` starts

| Piece | What |
|-------|------|
| `db` | `postgres:18-trixie` by digest, on `backend` only, role script `deploy/postgres/initdb.d/10-roles.sh` mounted read-only |
| `consumer` | Python 3.13 image, on `client-net` and `ctl-net` only (no route to `backend`/`proxy`), read-only rootfs, no capabilities |
| `e2e-ctl` | host process (not a container) bound to the `ctl-net` gateway, started by `run.sh` |

Secrets (`postgres_password`, the two role URLs, the ctl token) are generated per run into a temp directory and deleted at
teardown. Service and network names follow the canonical names (`proxy`, `backend`, service `worker`), not the
`worker-supervisor` / `proxy-net` / `backend-net` spelling of `environment.md`. The SUT, mocks and shaper are not declared
yet; they join the compose file with the epics that make them healthy.

## Reports (`e2e-results/`, gitignored)

- `report.csv`: `Test ID, Test Name, Execution Time (ms), Result, Error Message`; Result is PASS, FAIL or SKIP. Collection
  and setup errors are FAIL rows.
- `not-run.csv`: `Test ID, Code, Reason` for every NOT-RUN test.
- `metrics.json`: counts, duration, versions, reference-hardware ids (empty in the skeleton).
- `ctl-audit.log`: one JSON line per `e2e-ctl` call (refused calls too); `e2e-ctl.log`: the stub's own output.
- The end-of-run summary prints `PASS=n FAIL=n NOT-RUN=n` and one `NOT-RUN(<code>): n` line per code. Any FAIL makes the
  run exit non-zero.

The consumer always runs `pytest -v --tb=short -p e2e_report`, verbose. Never add quiet flags.

## NOT-RUN rule

A test that cannot run is never PASS:

    @pytest.mark.not_run("NO-SERVICE", "the api service is not part of the core profile yet")
    def test_something(): ...

or `e2e_report.skip_not_run(code, reason)` inside a test. Both give SKIP with the message `NOT-RUN(<code>): <reason>`. A
plain `skip` is counted separately in the summary and `metrics.json` (`skip_without_not_run_code`).

Codes: the hardware classes of `environment.md` (`HW-REF`, `HW-DEVICE`, `H-WIN`, ...) plus **`NO-SERVICE`**, "the service this
test needs is not part of the profile / build yet".

> **Open item for the test owner (E-31):** `environment.md` defines no code for "service not available in this build yet".
> `NO-SERVICE` is the skeleton's choice; confirm it or name another, then update `environment.md`.

## Self-tests (`tests/consumer/selftests`)

- `unit/`: the reporter on synthetic pass, fail, skip, setup error, collection error and the NOT-RUN marker (no stack needed;
  on a host run `PYTHONPATH=tests/consumer pytest -v --tb=short -p e2e_report tests/consumer/selftests/unit` with
  `E2E_RESULTS_DIR` set).
- `stack/`: the consumer cannot resolve `db` nor open TCP to the database address; every `e2e-ctl` action answers
  `not_implemented` with one audit line each; no token is refused and audited.
- `test_not_run_sentinel.py`: one test marked `NOT-RUN(NO-SERVICE)`; it proves the rule on every run and shows that the
  skeleton does not cover the SUT.
- `test_seeded_failure.py`: collected only with `SEED_FAIL=1`.
- `tools/e2e-ctl/test_e2e_ctl.py`: stub unit tests, run by `run.sh` on the host (`python3 -m unittest -v` in that directory).

## e2e-ctl stub

`tests/tools/e2e-ctl/e2e_ctl.py`, Python stdlib only. `POST /v1/actions/<name>` with `Authorization: Bearer <token>`.
Typed JSON errors `{"error": {"code", "message", "action"}}`: `not_implemented` (501) for every action in `actions.json`,
`unauthorized` (401) without or with a wrong token, `unknown_action` / `not_found` (404), `method_not_allowed` (405). The
audit line is written before the response. A wildcard bind address is refused.

`actions.json` lists the 22 rows of the Fault injectors table in kebab-case. `environment.md` names four of them itself
(`inspect`, `internal-http`, `alternate-key`, `exec-operator`); the other names are derived from the row titles. The real
names are an open item for E-31. The "account purge" teardown action named in the Isolation section is not a row of that
table and is not listed.

## reset

After the database is healthy, `run.sh` stops it, copies the `pgdata` volume to a second volume (golden snapshot) and
starts it again. `reset` does the reverse copy. The XFS blob-volume snapshot is E-31's and is not covered.
