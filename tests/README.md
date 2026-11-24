# Black-box test harness (AZ-3742; the system under test joined with E-31)

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
| `volumes-init` | one-shot: gives the blob volume to the api uid and opens the socket and scratch volumes to the api, worker and decoder uids |
| `migrate`, `keycheck` | one-shots of the api test image (`migrate`, `admin keycheck-init`), run before the golden snapshot is taken, so a reset restores a migrated, key-checked database |
| `mock-idp` | `tests/mocks/idp` as a container; `idp.test:9000` on `client-net` and `backend`; approves anyone, never published |
| `api`, `worker`, `decoder-interactive`, `decoder-general` | the SUT: the api test image (feature `test-clock`), the worker test image and the hardened decoder image with its seccomp profile, as in `deploy/compose/compose.dev.yml` |
| `caddy` | TLS 1.3 and HTTP/2 in front of the api (`sunnycloud.test` on `client-net`), with `deploy/compose/Caddyfile.dev` until AZ-4021 provides the release file; its internal CA root is exported to `e2e-results/certs/root.pem`, which the consumer trusts |
| `consumer` | Python 3.13 image, on `client-net` and `ctl-net` only (no route to `backend`/`proxy`), read-only rootfs, no capabilities |
| `e2e-ctl` | host process (not a container) bound to the `ctl-net` gateway, started by `run.sh` |

Secrets (`postgres_password`, the two role URLs, the master key and the file that names it, the worker token, the share grant key, the mock identity provider's client secret, the ctl token) are generated per run into a temp directory and deleted at
teardown. Service and network names follow the canonical names (`proxy`, `backend`, service `worker`), not the
`worker-supervisor` / `proxy-net` / `backend-net` spelling of `environment.md`. The WAN shaper (needs `NET_ADMIN`; the timing tests run on the reference host), `mock-update-server` and the profiles
other than `core` are not declared yet.

`E2E_KEEP_ON_FAILURE=1 tests/run.sh --profile core` leaves the stack up when the run fails, so the container logs can be read; remove it with `tests/run.sh down`. The first run builds the api, worker and decoder images (the decoder builds ffmpeg and libheif from source) and takes a long time; later runs use the layer cache.

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
- `stack/test_sut_edge.py`: the front door: `server-info` through Caddy over the exported root certificate, TLS 1.3, the
  mock identity provider's issuer, and that no backend service name resolves from the consumer.
- `test_not_run_sentinel.py`: one test marked `NOT-RUN(NO-SERVICE)`; it proves the NOT-RUN rule on every run.
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
