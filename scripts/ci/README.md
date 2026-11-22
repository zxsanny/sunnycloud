# CI check scripts

CI YAML only calls these scripts (`_docs/04_deploy/ci_cd_pipeline.md`, Principles). Run them locally the same way.

## One-time setup: sqlx-cli

The command-line tool must match `sqlx = "0.9"` (workspace `src/Cargo.toml`). Pinned: `0.9.0`.

```bash
cargo install sqlx-cli --locked --version 0.9.0 --no-default-features --features rustls,postgres
sqlx --version        # sqlx-cli 0.9.0
cargo sqlx --version  # sqlx-cli-sqlx 0.9.0
```

Both scripts refuse to run on any other minor version. Both need a PostgreSQL 18 reachable through `DATABASE_URL`:

```bash
docker run -d --name sunnycloud-pg18 -e POSTGRES_PASSWORD=dev -p 127.0.0.1:5432:5432 postgres:18 -c max_connections=400  # the workspace suite holds ~270 at peak
export DATABASE_URL=postgres://postgres:dev@127.0.0.1:5432/postgres
```

## PostgreSQL 18 client for the restore tests: `SC_PG_TOOLS`

`src/server/tests/pipeline_db_restore.rs` (AZ-3666) runs a real `pg_dump` and `pg_restore` of its test database. The tools come from the directory named by `SC_PG_TOOLS`, or from `PATH` when it is unset; they must be major version 18, and a missing tool fails the test (it is never skipped). They connect through `PGHOST`, `PGPORT`, `PGUSER`, `PGPASSWORD` and `PGDATABASE`, which the tests set from `DATABASE_URL`; the dump goes to stdout and the restore reads stdin.

```bash
scripts/ci/install-tools.sh pg-client-18                 # Ubuntu: apt (sudo), then
export SC_PG_TOOLS=/usr/lib/postgresql/18/bin
```

Without a local client, `SC_PG_TOOLS` may name a directory of wrappers called `pg_dump` and `pg_restore` that run the tool of the same name in the database container, forwarding only `PGUSER`, `PGPASSWORD` and `PGDATABASE` (the container's own socket replaces host and port):

```bash
#!/usr/bin/env bash
exec docker exec -i -e PGUSER -e PGPASSWORD -e PGDATABASE sunnycloud-pg18 "$(basename "$0")" "$@"
```

## `check-migrations.sh`: up, down, up

Applies every migration to an empty database, reverts them all (newest first, one at a time), applies them again, and prints each step. It exits non-zero and names the version when an `up` or `down` file fails, when a file is not part of a reversible `YYYYMMDDHHMMSS_<verb>_<object>.up.sql` / `.down.sql` pair, or when the database already has applied migrations. With zero migrations it prints `0 migrations applied, reverted and re-applied`.

```bash
DATABASE_URL=... scripts/ci/check-migrations.sh
MIGRATIONS_DIR=/path/to/dir DATABASE_URL=... scripts/ci/check-migrations.sh   # default: src/migrations
```

The database is left in the fully migrated state. Drop and recreate it before the next run.

Before step 5 the script runs `ensure-db-roles.sh`: the first migration (`accounts_base`) grants to the role `sunnycloud`, and a plain PostgreSQL service has no application roles (the Compose database gets them from `deploy/postgres/initdb.d/10-roles.sh`). `ensure-db-roles.sh` creates `sunnycloud_migrator` and `sunnycloud` when missing, with the attributes of that init script and no password; it needs `psql` and a superuser `DATABASE_URL`. `rust-lint.sh` calls it before its own `sqlx migrate run`.

## `check-sqlx-prepare.sh`: offline query data is current

Runs `cargo sqlx prepare --check --workspace` in `src/` and fails when the committed `src/.sqlx/` data does not match the compile-time checked queries. The database behind `DATABASE_URL` must already have all migrations applied (run `check-migrations.sh` first, or `sqlx migrate run --source src/migrations`). To refresh the data after changing a query:

```bash
cd src && cargo sqlx prepare --workspace   # then commit src/.sqlx
```

Builds without a database use `SQLX_OFFLINE=true`, which reads `src/.sqlx/`. `WORKSPACE_DIR` overrides the workspace directory (used by the tests).

## Tests of the scripts

```bash
scripts/ci/tests/test-no-quiet-flags.sh                     # no database needed
DATABASE_URL=... scripts/ci/tests/test-check-migrations.sh        # needs CREATE DATABASE rights
DATABASE_URL=... scripts/ci/tests/test-check-sqlx-prepare.sh      # builds a scratch workspace under /tmp
```

The database tests seed their files only in temporary directories and create and drop their own databases.

## PR gate: `.github/workflows/ci.yml` and its scripts (AZ-3761)

The workflow only checks out, caches and calls these scripts, each under `watchdog.sh`. Jobs: `rust-lint`, `rust-test`, `pipelines-large`, `http-api-suite`, `pg18-migrations`, `supply-chain`, `image-build`, `images`, `decoder-suite`, `vault-core`, `mobile`; triggers: pull request to `dev`, push to `dev`.

| Script | Job | What it does |
|--------|-----|--------------|
| `setup-rust.sh` | all cargo jobs | `rustup toolchain install` from `src/rust-toolchain.toml`, verifies the pinned channel |
| `install-tools.sh TOOL...` | all cargo jobs | pinned `cargo-deny`, `cargo-llvm-cov`, `gitleaks` (SHA-256 verified downloads), `cargo-audit`, `sqlx-cli` (`cargo install --locked --version`); `pg-client-18` (`rust-test`): the PostgreSQL 18 client from the PGDG apt repository at an exact package version (bump it in the script when PGDG rotates the old version out of its index), its signing key accepted only with the pinned fingerprint, into `/usr/lib/postgresql/18/bin` (the job's `SC_PG_TOOLS`); the versions live in that script |
| `rust-lint.sh` | `rust-lint` | `cargo fmt --check`, clippy `-D warnings` over the workspace, then clippy `-D warnings` of `sunnycloud-server` with `--features vault` (the vault service is linked only with that feature) and with `--features test-clock` (the `-test` images' clock, AZ-4334), then clippy of `decoder-runner` with `--features test-freeze` (the stub binary of the decoder `test-stubs` image exists only with it), `sqlx migrate run`, `check-sqlx-prepare.sh` (needs `DATABASE_URL`) |
| `check-coverage.sh` | `rust-test` | `cargo llvm-cov test --workspace --locked` (lists every test), then 75% overall and 90% per entry of `critical-paths.txt` (an entry may carry its own minimum, `PATH MIN`); prints the measured values |
| (inline `cargo test`) | `rust-test` | after the coverage step: `cargo test --locked -p sunnycloud-server --features vault` over `vault_http`, `vault_report`, `local_commands`, `api_services`, `composition_root`, then `--features test-clock` over `test_clock_serve`, `pipeline_gc_retention`, `api_services`, `composition_root` (the feature-gated server tests the workspace run leaves off) |
| `run-one-test.sh CMD...` | `pipelines-large` | runs one `cargo test ... -- --ignored --exact NAME` (the 1 GiB download and worker ACs, release mode), passes its output through, and fails unless exactly one test passed: a name that matches nothing would otherwise exit 0 with `0 passed` |
| `check-migrations.sh` | `pg18-migrations` | see above |
| `check-it-suite.sh` | `http-api-suite` | runs IT-11-1..20 of `components/11_http_api/tests.md` through `it_suite.py` and the id-to-test map `src/components/11_http_api/tests/it_cases.txt`; prints each case with result and duration; fails on a case with no test, a prefix that selects nothing, an unknown id, a failing run or a case over its max execution time |
| `check-pinning.sh [ROOT]` | `supply-chain` | `uses:` = 40-hex SHA + `# vX.Y.Z`, images by digest, default `permissions: contents: read`, no fork checkout under `pull_request_target` |
| `check-licence.sh` | `supply-chain` | `LICENSE` matches `licence.sha256`; `COPYING.iOS` checked once it exists |
| `audit.sh` | `supply-chain` | validates the dated ignores in `src/deny.toml` (`added`, `expires`, `why`; not expired; `added` at most 90 days old), passes the same ids to `cargo audit --deny warnings`, runs `cargo deny check advisories bans licenses sources`. `DENY_TOML` and `AUDIT_TODAY` override the file and the date |
| `build-wasm.sh` | `supply-chain` | wasm32 build of `sync-core-wasm` and `sc_ids` |
| `secret-scan.sh` | `supply-chain` | gitleaks over the full history with `gitleaks.toml` (built-in rules plus a task-id allowlist); findings name the file, values are redacted; a shallow clone is refused |
| `build-images.sh` | `image-build`, `images` | `build NAME FILE` builds one of the five images (api, worker, decoder, api-test, worker-test; one matrix leg each) and saves it; `check` runs `deploy/tests/check-images.sh` on the five loaded images; without arguments it does both locally; never pushes |
| `decoder-suite.sh IMAGE` | `decoder-suite` | builds the `test-stubs` variant (`deploy/docker/decoder.Dockerfile --target test-stubs`, `TEST_STUBS_BASE=IMAGE`: a Rust build of the runner with `test-freeze` and the stub binary plus one layer, no second media build) and runs `cargo nextest run -p decoder-runner --run-ignored only -E 'binary(container)'`: the H3 cases of `components/10_decoder/tests.md` (AZ-3865), each named `it_10_<n>_...`, against the production flags of the dev Compose file (`DECODER_IMAGE`, `DECODER_TEST_IMAGE`); needs the host reference tools (`install-tools.sh media-tools`); removes its containers (label `sunnycloud-suite=h3`) on exit; never pushes |
| `watchdog.sh [--name N] -- CMD...` | every long step | heartbeat every 30 s (`WATCHDOG_TICK`), STALLED and abort of the whole process group after 120 s without output (`WATCHDOG_STALL`), exit 124 |

Tests: `tests/run-script-tests.sh` (run by `supply-chain`) covers the pinning, audit, coverage-threshold, IT-suite runner, one-test runner, watchdog (fast), secret-scan and quiet-flag checks. Run separately: `tests/test-watchdog-realtime.sh` (about 2.5 minutes, default 30 s / 120 s) and `tests/test-check-coverage-real.sh` (needs `cargo-llvm-cov`).

Advisory ignore format in `src/deny.toml`: `{ id = "RUSTSEC-YYYY-NNNN", reason = "added=YYYY-MM-DD; expires=YYYY-MM-DD; why=..." }`.

## `check-wasm-vectors.sh`: native, WASM and UniFFI give the same results

Builds `sync-core-wasm` with wasm-pack and runs the shared vector file (`src/sync-core-wasm/vectors/sync_core_vectors.txt`) through the native, Node, browser (Chromium, Firefox, WebKit through Playwright) and UniFFI legs; prints `LEG <name> PASS|FAIL|NOT-RUN(reason)` per leg and fails on any difference, naming the case (contract `13_sync_core/wasm_exports.md`). Needs wasm-pack 0.15.x and node; Playwright from `PLAYWRIGHT_DIR` or the npx cache. The UniFFI leg reports `NOT-RUN(EMU)` until an emulator harness exists.

```bash
scripts/ci/check-wasm-vectors.sh                      # browsers that cannot launch are NOT-RUN
scripts/ci/check-wasm-vectors.sh --require-browsers   # a browser that cannot launch fails the gate
scripts/ci/tests/test-check-wasm-vectors.sh           # a seeded one-bit difference must fail and name the case
```

## Mobile app and native library: the `mobile` job (AZ-4002, E-27/c18)

The Flutter project is `src/mobile/` (Kotlin layer in `android/`, Dart in `lib/`, `test/`). The one Rust library per ABI is `mobile_backup` with feature `uniffi` (sync-core inside); Gradle packages it as `libmobile_backup.so` from `src/mobile/android/app/src/main/jniLibs/<abi>/` (build output, gitignored), and the app loads it through JNA (UniFFI) and `System.loadLibrary` (`TlsTrustBridge`). The versions (NDK `28.2.13676358` at API 30, build-tools, Flutter) are pinned in `mobile-common.sh`; JDK 17 or newer comes from the caller (`actions/setup-java`, Temurin 21). Every script prints each step, runs `cargo` and `flutter` verbosely, and exits non-zero on a failed gate. `ANDROID_HOME` must name the Android SDK; `flutter` and `java` must be on `PATH`.

| Script | What it does |
|--------|--------------|
| `mobile-setup.sh` | Flutter 3.47.7 (download verified against a pinned SHA-256, into `MOBILE_TOOLS_DIR`, default `~/.cache/sunnycloud-mobile-tools`) and, with `sdkmanager`, the NDK, platform 35 and build-tools 36.0.0 |
| `mobile-build-native.sh` | `rustup target add`, then `cargo build --release --locked -p mobile_backup --features uniffi --target T` for `aarch64-linux-android`, `armv7-linux-androideabi` and `x86_64-linux-android` with the NDK clang wrappers at API 30 (the 16 KB link flag is in `src/.cargo/config.toml`), copied to `jniLibs/<abi>/` |
| `mobile-check-alignment.sh PATH...` | AC-2: every LOAD segment of every `.so` (file, directory, or the `lib/*/*.so` of an APK, ours, JNA's and Flutter's) is at least `0x4000` (`llvm-readelf -lW`), plus `zipalign -c -P 16 -v 4` on an APK; fails when below or when nothing was checked |
| `mobile-check-bindings.sh [--update]` | AC-3: builds the host cdylib and `uniffi-bindgen` (feature `uniffi-cli` of `mobile_backup`, uniffi 0.32.2, never in a release build), regenerates the Kotlin bindings (library mode, `--no-format`) and diffs them against `src/mobile/android/app/src/main/kotlin/uniffi/`; `--update` rewrites them (commit the result after every change of the Rust surface). `BINDINGS_DIR` and `SRC_DIR` override the directories |
| `mobile-check-no-vault.sh` | AC-4: `cargo tree --target T --edges normal,build` of `mobile_backup` with feature `uniffi` for each ABI; fails when `vault-core` is linked, or `sync-core` is not (an empty listing cannot pass). `SRC_DIR` overrides the workspace |
| `mobile-build-app.sh debug\|release` | AC-1: `flutter build apk -v` over the built libraries (Gradle fails when one is missing), then `mobile-check-apk.sh`. Release without `android/key.properties` is signed with the debug key and is not shippable (the Play keys are the owner's; the file is gitignored and names `storeFile`, `storePassword`, `keyAlias`, `keyPassword`) |
| `mobile-check-apk.sh APK` | `minSdkVersion` 30, no `android:process` in the manifest (one process), `lib/<abi>/libmobile_backup.so` for every ABI, then `mobile-check-alignment.sh` |
| `mobile-test.sh` | `flutter analyze`, `flutter test -v`, host cdylib, and the Kotlin unit tests (`gradlew :app:testDebugUnitTest`) that load it through JNA and call `BackupCore.status` without a server |
| `tests/test-mobile-checks.sh` | the seeded failures: a 4 KB-aligned library fails the alignment check, a renamed `BackupCore.status` in a copy of the committed bindings fails the drift check, a copy of the workspace where `mobile_backup` depends on `vault-core` fails the no-vault check; each must also pass its real input and fail for the seeded reason. Needs the NDK, cargo and the jniLibs of `mobile-build-native.sh` |

Laptop run, in the job's order (from the repository root):

```bash
export ANDROID_HOME=~/Android/sdk JAVA_HOME=/path/to/jdk21 PATH=/path/to/flutter/bin:$JAVA_HOME/bin:$PATH
scripts/ci/mobile-setup.sh && scripts/ci/mobile-build-native.sh && scripts/ci/mobile-check-no-vault.sh
scripts/ci/mobile-check-bindings.sh && scripts/ci/mobile-build-app.sh debug && scripts/ci/mobile-build-app.sh release
scripts/ci/mobile-test.sh && scripts/ci/tests/test-mobile-checks.sh
```

The device error for a 4 KB library on a 16 KB device (`dlopen failed: empty/missing DT_HASH/DT_GNU_HASH`) does not name alignment, so `mobile-check-alignment.sh` is the diagnostic. The emulator harness (E-27/c19) and the iOS static library (E-28/c03) are not part of this job.
