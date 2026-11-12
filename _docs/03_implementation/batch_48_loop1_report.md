# Batch Report

**Batch**: 48
**Tasks**: AZ-3747_blobstore_perf_security (done: ST-04-8 and the block-rewrite rule), AZ-3916_updater_verification (done; release keys are an owner blocker)
**Date**: 2026-11-12

Two parallel workers: the blobstore plus the `BlobError` contract file, and the desktop daemon (the only lockfile owner; it added no package). The parent recorded four owner decisions in the docs, reviewed, ran the gate and committed.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3747 (AC-10, ST-04-8) | Done | `contracts::BlobError::BlockConflict` added. The port's `write_block` now runs `write_block_once` under a per-block lock (256 striped mutexes keyed by blob id and index, held from the read to the write). It reads the block's ciphertext region and acts on it. All zero: a first write. Every chunk authenticates and equals the new plaintext: the stored ciphertext is written back unchanged (owner, 2026-11-12) and never re-sealed. Different bytes, or a torn region that does not authenticate: `BlockConflict`, nothing written. 7 ST-04-8 tests: identical, different, one-bit flips at first/middle/last byte, 8 rounds of concurrent A/B writers, re-send after a crash, torn region, never-written index. The write-back is proved by the file's mtime and checked by removing it once. The rest of AZ-3747 was done in batch 47; the gates not run on this host are in `_process_leftovers/2026-11-12_az-3747-not-run-gates.md` (owner option A) |
| AZ-3916_updater_verification | Done | Fetch, verify (signature against either pinned key, `format`, `expires`, `serial` above the one in `meta`, `version`, platform entry, size then SHA-256), download by asset file name, stage and report "update ready" (owner option A: the user installs it). The eight DS-31 bad builds are each rejected by their own check. Env and settings overrides are ignored. Downgrade and key confusion are tested (ST-14-3). Redirects are followed only to `github.com`, `release-assets.githubusercontent.com` and `objects.githubusercontent.com`, over HTTPS, at most 3 hops (owner, 2026-11-12). The offline `update-sign` tool and its refusals are built. Test keys are derived inside the tests; no real key was created |

## Owner decisions (2026-11-12)

1. An identical block re-send writes the stored ciphertext back unchanged, so a re-send after a failed `fdatasync` really reaches the disk. The blobstore description and the blob-writer contract are updated.
2. Release downloads follow redirects only to a compiled-in GitHub host list (ADR-017 updated).
3. The two release public keys are created offline before the first desktop release. Release builds refuse every manifest until then (`_process_leftovers/2026-11-12_desktop-updater-release-keys.md`).

## Code Review Verdict: PASS

No findings on the rewrite check or the redirect policy.

CI-parity (`CARGO_INCREMENTAL=0`): fmt, workspace clippy `-D warnings`, workspace tests on PostgreSQL 18 (2967 passed, 0 failed, 19 ignored), wasm build and the four check scripts all exit 0.

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | The block lock lives in one `FileBlobWriter`, so the composition root must build exactly one per process | 04 / E-22/c03 | Note for the composition root task | unclear |
| 2 | Every port `write_block` now reads its region first (about 2 MiB, normally from the page cache); a rewrite also decrypts 32 chunks (about 1 ms) | ST-04-8 | Accepted cost; `write_complete` pays nothing | clear |
| 3 | The GitHub host list was chosen from known behaviour and not checked against a live release | AZ-3916 | Confirm with a real release download once keys and releases exist | unclear |
| 4 | `desktop.update_check_interval` has no row in the `sc_config` key table; `UpdaterConfig` defaults to 24 h | AZ-3916 | Follow-up in helper 01 | unclear |
| 5 | `ipc_schema::UpdateView` carries only the version, so the UI has no staged file path to open | AZ-3916 | Follow-up in the settings window task | unclear |
| 6 | `updater_source` and `updater_public_key_1/2` rows in `sc_config` are never read by the updater | AZ-3916 | Candidates for removal | clear |
| 7 | Once the newest version is installed, each daily check logs a WARN (`version` or `replay`), as specified | AZ-3916 | As specified | clear |

## Commit

`[AZ-3747] [AZ-3916] Block-rewrite rule (BlockConflict) and the desktop updater` — Loop: 1

## Next Batch

AZ-4231 (device login wire and unavailable credentials), then the remaining ready tasks.
