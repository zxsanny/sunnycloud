# MVP build: doc and config drifts found on 2026-11-20

Found while building the MVP stack (workers A and E). None blocks the MVP; each needs a docs or dev-compose fix.

| # | Where | Drift | Fix |
|---|---|---|---|
| 1 | `_docs/02_document/deployment/environment_strategy.md` | Says the server prints a setup token at first start. It does not: `start.rs` never calls `ensure_setup_token`; only `sunnycloud admin setup-token` creates one | Change the text to the CLI path |
| 2 | `deploy/compose/compose.dev.yml` header | The secret `share_grant_key` is generated with `openssl rand -out ... 32` (raw bytes), but the loader reads secrets as UTF-8 text with a 32-byte minimum, so raw bytes can fail | Generate 64 hex characters, as `scripts/dev/mvp-secrets.sh` does |
| 3 | `deploy/compose/compose.dev.yml` | No `master_key_file` secret is mounted; the code reads the key from the file named by that secret (04 keyring) | Mount it as the MVP compose does |
| 4 | `sunnycloud doctor` on the host | Reports "the decoder image is 20777 days old" because a host process has no build date | Expected until the container media stage (AZ-3863, AZ-3864); doctor could skip the check for a non-container decoder |
| 5 | `mock-idp` | Named in `compose.dev.yml`, `tests/compose.test.yml` and the dev-seed code, but not in the repo | Build it (`tests/mocks/idp`) with the E-31 stack, or drop the mentions |
| 6 | Decoder container | `decoder-runner` now refuses to start when `vipsthumbnail`, `vipsheader`, `ffmpeg` or `ffprobe` is missing from PATH. The container image has none of them, so decoder containers exit at start in `compose.dev.yml` and the E-31 test stack until AZ-3863 and AZ-3864 land | Land the media stage, or make the dev compose use the host decoder |
| 7 | Decoder container (AZ-3863, AZ-3864) | On Ubuntu 24.04 `libheif1` decodes only AVIF by default: an iPhone HEIC (HEVC) needs `libheif-plugin-libde265`, and test files need `libheif-plugin-x265`. Without the first every HEIC answers `Corrupt`, which the worker treats as a permanent failure. The media stage must install both; a capabilities or self-check item for HEVC decode (E-19/c12) would make a missing plugin visible | Install the plugins in the media stage and report HEVC decode in the self-check |
| 8 | Decoder tool flags | libvips 8.15 (Ubuntu 24.04) and 8.18 differ: `vipsheader -f a -f b` prints one value on 8.15, and `vipsthumbnail` flags differ. The decoder now uses `vips thumbnail` and one `vipsheader` call per field, which behave the same on both | None; keep a CI run on 24.04 |
