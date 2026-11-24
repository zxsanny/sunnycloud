# Batch Report

**Batch**: 114
**Tasks**: the E-31 compose test stack, slice 1 (no ticket: the harness skeleton AZ-3742 is archived; the suites AZ-3654, 3661, 3662, 3664, 3665, 3815, 3835, 3836, 3847, 3848, 3878, 3879, 3971 plug into it). Owner choice of 2026-11-24: the whole stack in one go.
**Date**: 2026-11-24

## What changed

`tests/run.sh --profile core` now starts the system under test, not only the database:
- `tests/compose.test.yml`: `volumes-init`, `migrate` and `keycheck` (one-shots of the api test image), `api` (feature `test-clock`), `worker`, `decoder-interactive` and `decoder-general` (release image, seccomp profile, no network), `mock-idp`, `caddy` (TLS 1.3, HTTP/2, h2c to the api; `deploy/compose/Caddyfile.dev` until AZ-4021), and the consumer behind Caddy. Names: `sunnycloud.test` and `idp.test` on `client-net`; the api reaches `idp.test` on `backend`. Test durations are short, from config.
- `tests/run.sh`: more generated secrets (master key and the file that names it, worker token, share grant key as 64 hex characters, the identity provider's client secret), image builds, the one-shots before the golden snapshot (a reset now restores a migrated, key-checked database and restarts api and worker), export of Caddy's root certificate to `e2e-results/certs/root.pem` for the consumer, and `E2E_KEEP_ON_FAILURE=1` to keep a failed stack for its logs.
- `tests/mocks/idp`: a Dockerfile (bun image by digest), an opt-in `MOCK_IDP_BIND_HOST` (default stays loopback; the container sets `0.0.0.0` on isolated networks only) and `MOCK_IDP_CLIENT_SECRET_FILE`. 18 bun tests pass, typecheck clean.
- `tests/consumer/selftests/stack/test_sut_edge.py`: four checks of the front door (server-info through Caddy over the exported root, TLS 1.3, the identity provider's issuer, no backend name resolves from the consumer). The NOT-RUN sentinel now says what it is.

## Defects of the existing configuration found on the way (all were in the stack's wiring, none in product code)

- `blob_root`, `master_key_file` (a secret whose text is the key file's path), `share_grant_key` (hex text, not raw bytes) and `decoder_gid` are required by the server and worker configuration; `compose.dev.yml` sets none of them, so the dev stack cannot start the api or the worker as written. Not fixed there (out of this batch's paths); recorded in the leftover below.
- Named volumes are root-owned while the services run as 10001, 10002 and 10003; the worker needs `group_add 10003` to open the decoder socket (mode 0660).

## Code Review Verdict: not run as a separate pass

Configuration and scripts only; the stack running green is the proof. `shellcheck tests/run.sh`: one old warning (SC2155 at the `export`). `check-pinning.sh`: 0 unpinned. `deploy/tests/lint.sh`: one old failure (`decoder.Dockerfile` `FROM ${TEST_STUBS_BASE}` has no digest), not from this batch.

## Test Suite

`tests/run.sh --profile core`: PASS=19 FAIL=0 NOT-RUN=1 (the sentinel), exit 0, three times in a row after the last fix; `tests/tools/check-reset.sh` on a kept stack: reset in 8 s (budget 60 s). The first run builds the images from source (ffmpeg, libheif, the Rust workspace twice) and takes about 6 minutes; later runs use the layer cache.

## Not covered

- The WAN shaper (`tc netem`, needs `NET_ADMIN`; timing tests belong to the reference host), `mock-update-server`, the profiles other than `core`, a release-image api beside the test image (IT-11-9 on the release image), the release Caddyfile (AZ-4021).
- Not wired into CI: the job would build five images and run the stack on a runner; to be decided with the first suite that needs it.

## Discovered during implementation

| Spec ref | Proposed handling | clear/unclear |
|----------|-------------------|---------------|
| `deploy/compose/compose.dev.yml` | Add `blob_root`, `master_key_file`, hex `share_grant_key`, `decoder_gid`, `group_add` and the volume ownership step, or drop the dev worker; same fixes as in the test stack | clear |
| `environment.md` lists `edge-net` and `wan-shaper` | Slice 1 puts Caddy and the identity provider on `client-net`; moves to `edge-net` when the shaper joins | clear |

## Commit

`[no-ticket] E-31 compose test stack: api, worker, decoders, Caddy and the mock identity provider behind run.sh --profile core` - Loop: 1

## Next: the suites that plug into the stack: AZ-3835 Mode R (IT-11-9, IT-11-17), AZ-3654 first run (setup token through the stack), AZ-3664 sign-in, AZ-3662 share links, AZ-3665 account deletion
