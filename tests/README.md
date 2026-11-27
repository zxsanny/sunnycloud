# Black-box test harness (AZ-3742; the system under test joined with E-31)

Design: `_docs/02_document/tests/environment.md`. This directory holds the shared entry command, the test compose file, the
Python consumer with its CSV reporter, and the `e2e-ctl` stub. Pipeline tasks and the E-31 scenarios plug into it.

## Run

    tests/run.sh --profile core            # start db + consumer, run the self-tests, tear everything down
    tests/run.sh --profile core --keep     # same, but leave the stack up
    tests/run.sh --profile h2              # the core stack with the worker-suite timings (harness H2)
    tests/run.sh reset                     # restore the database and blob volumes to the golden snapshot (stack must be up)
    tests/run.sh down                      # tear down a kept stack
    SEED_FAIL=1 tests/run.sh --profile core   # adds one failing self-test; the run must exit non-zero
    tests/tools/check-reset.sh             # needs --keep: writes rows, resets, checks they are gone and the time

Needs on the host: Docker with Compose, `openssl`, `python3`. The `core` run takes about 30 s.

Profiles: `core`, `h2`, `web`, `desktop`, `android-emu`, `perf`, `lan`, `gpu`. `core` and `h2` have services; the others exit 3
with "not available yet" (never a pass). An unknown name prints the usage and exits 2. `h2` is harness H2 of the worker
tests (`_docs/02_document/components/09_worker_supervisor/tests.md`): the `core` services with `tests/compose.h2.yml` merged
over them (job lease 6 s, claim poll 100 ms, status 1 s, decoder silence 1 s, cancel grace 500 ms, photo timeout 5 s, shutdown
grace 3 s). The profile of a kept stack is in `e2e-results/.stack-state`, so `reset`, `rerun` and `down` use its files.
`selftests/stack/test_worker_harness.py` checks under either profile that the api and the worker run with its timings.
`h2` also declares FD, the scripted fake decoder (`src/worker/fake-decoder`, image target `fake-decoder` of
`deploy/docker/worker.Dockerfile`; script and record formats in the crate's module doc), as services `fd-interactive` and
`fd-general` (compose profile `fd`, never started by `up`). A suite takes a lane with `fdkit.FakeDecoderLane(ctl, lane)`:
`start(script)` writes `e2e-results/fd/<lane>/script.json`, removes the last `record.jsonl` and has e2e-ctl stop the real
decoder and start FD on its socket and scratch volumes; `record()` / `wait_for(kinds, seconds)` read what FD received
(a timeout adds FD's log); `stop()` puts the real decoder back. Both restart the supervisor unless `restart_worker=False`, so a case does not
wait out the reconnect backoff (1 s to 30 s); IT-09-15 itself passes `False`. FD is for fault and protocol cases only,
never for a media outcome.
`suites/worker/test_fake_decoder_stack.py` (AZ-4382 AC-3) runs under `h2` and is skipped under `core`.

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
- `stack/`: the consumer cannot resolve `db` nor open TCP to the database address; every `e2e-ctl` action answers a typed
  error with one audit line each (`bad_request` for the implemented ones given an empty body, `not_implemented` for the
  rest); no token is refused and audited.
- `stack/test_sut_edge.py`: the front door: `server-info` through Caddy over the exported root certificate, TLS 1.3, the
  mock identity provider's issuer, and that no backend service name resolves from the consumer.
- `test_not_run_sentinel.py`: one test marked `NOT-RUN(NO-SERVICE)`; it proves the NOT-RUN rule on every run.
- `test_seeded_failure.py`: collected only with `SEED_FAIL=1`.
- `tools/e2e-ctl/test_e2e_ctl.py` and `test_ctl_actions.py`: ctl unit tests, run by `run.sh` on the host (`python3 -m unittest -v` in that directory).

## Suites (`tests/consumer/suites`)

Test cases of the component test specs, run against the stack. `suites/http_api/`: IT-11-9 (listener placement on the test
image, AZ-3835 AC-2) and IT-11-17 (readiness through a database outage and a pending migration, AC-3). They reach the
listeners Caddy does not route (`:8081`, `:9090`, `:9091`) through the ctl action `internal-http` (`stackkit.py`).
The release-image leg of IT-11-9 is not run: the stack carries the test image only (see the leftover of AZ-3835).

`suites/media/` (AZ-3661, flow F10): `test_media_pipeline.py` commits a JPEG with EXIF orientation 6, three clips (above the
bitrate limit, compliant with the index first, compliant with the index last), a truncated HEIC and an SVG named `.png`, and
reads what the real supervisor and decoders made of them through the thumbnail, transcode and thumbnail-states routes and
`doctor`. `test_worker_crash.py` (AC-71) freezes both decoders (`process-kill pause`), kills the supervisor while a job is
in a decoder, starts it again, moves the test clock past the 10 s lease and counts what is left: one row per derived kind,
no unreferenced blob after the derived janitor, empty scratch volumes. NOT-RUN: a hostile-class decoder exit (needs the
decoder test image), and the supervisor kill during the input fetch and during the output post (no hold on those phases).
The files come from `tests/tools/gen_media.py`, which `run.sh` runs with the host's `ffmpeg` into `e2e-results/media`.
`suites/operator_cli/` (AZ-3815): AT-12-1 (`doctor --pre-flight` prints the provider's redirect URI, the first admin,
`doctor` exit 0), ST-12-1 (each one-time token once on its creating stdout, never in a stderr, a SUT log, an audit row or
host `ps`; the master key nowhere), ST-12-2 (`admin user delete`, `rewrap`, `epoch-bump`, `migrate revert` without `--yes`
exit 3 and change no row, migration or epoch), ST-12-3 (`doctor` and `scrub run --full` open no listening socket in the
api's network namespace) and ST-12-4 (the api role is refused `DROP TABLE` and `CREATE TABLE`, the api holds no migrator
secret). `test_restore_drill.py` is AT-12-2: a dump, an epoch bump, the dump loaded under a stopped api, `serve` refusing
with the bump named, `doctor` failing on the epoch, the bump, `scrub run --full`, `doctor`, ready, and a device cursor of
before the dump told to resync. PT-12-1 and PT-12-2 are NOT-RUN(HW-REF).
`tests/run.sh rerun <pytest args>` runs the consumer against a kept stack without starting or stopping it; a change in
`tests/tools/e2e-ctl/` needs a new stack (the ctl process is started once).

## e2e-ctl

`tests/tools/e2e-ctl/e2e_ctl.py`, Python stdlib only. `POST /v1/actions/<name>` with `Authorization: Bearer <token>` and a
JSON body of parameters. Implemented (`ctl_actions.py`): `internal-http` (one HTTP request to a listener of the api on the
backend network, or to the worker's ops port `:9090` with `service: "worker"`; `lines_with` keeps only the lines of
the answer holding a text, read whole first, as for a metrics page; `auth: "worker"` adds the worker bearer on
the api's listeners only, and it never leaves ctl), `db-outage` (`op` `stop` or `start`,
start waits for a healthy database), `interrupted-upgrade` (`op` `revert` with `steps`, or `apply`, through the product's
own `migrate`), `process-kill` (`kill` or `start` of the api, the worker or a decoder, whose runner is the container's main process;
`pause` or `unpause` of a decoder; `fake` or `real` of a decoder (h2: FD in its place, or the decoder back); `stop` of the worker (SIGTERM, then its exit code and how long it took); `restart` of the api, waiting for it to be healthy, with `worker_token: "next"` (a second
accepted token in `worker_api_token_next`) or `"drop-old"` (the next or a fresh token becomes the only one: a worker still
holding the old one gets 403 until it restarts); the secret files are rewritten in place and no token is returned),
`inspect` (a container's state, start time and restart count, its log tail, a decoder's processes listed by `docker top`,
the timing settings the api or the worker was started with, the top-level names in a decoder scratch volume, a secret
scan that returns only counts, or the api role's DDL rights and whether it holds a migrator secret), `db-probe` (fixed
queries, among them the media jobs and derived rows), `db-restore` (the golden snapshot, or a named `pg_dump` and its
load), `exec-operator` (an operator command in the api container, in a one-off api container while `serve` is down, or
`migrate ...` in a one-off `migrate` container; optionally sampling the api's listening sockets while it runs), `scratch-fill` (leftover job trees in a decoder scratch volume), and the worker suite's input faults in `ctl_worker.py`: `bit-flip` (one bit of a blob's first chunk), `job-row-edit` (one bit of a queued job's `content_hash`) `worker-variant` (the worker recreated with a file of `tests/h2-variants/`, or as the stack defines it) and `cpu-limit` (a decoder's CPU quota, with the one it had in the answer). `inspect` also lists a container's environment names, secrets, mounts and media tools, and filters a log to the lines holding a text (`lines_with`). The audit line names the action and status, never the body.
Typed JSON errors `{"error": {"code", "message", "action"}}`: `not_implemented` (501) for every other action in `actions.json`,
`unauthorized` (401) without or with a wrong token, `unknown_action` / `not_found` (404), `method_not_allowed` (405). The
audit line is written before the response. A wildcard bind address is refused.

`actions.json` lists the 26 rows of the Fault injectors table in kebab-case. `environment.md` names four of them itself
(`inspect`, `internal-http`, `alternate-key`, `exec-operator`); the other names are derived from the row titles. The real
names are an open item for E-31. The "account purge" teardown action named in the Isolation section is not a row of that
table and is not listed.

## reset

After the database is healthy, `run.sh` stops it, copies the `pgdata` and the `blobs` volumes to two more volumes (the
golden snapshot) and starts it again. `reset` stops api, worker and database, does the reverse copies and starts them. The
blob volume goes back with the database because the api refuses a database whose epoch is below the blob volume's `.epoch`
marker (a restore without `admin epoch-bump`). The XFS blob-volume snapshot of the reference host is E-31's and is not covered.
