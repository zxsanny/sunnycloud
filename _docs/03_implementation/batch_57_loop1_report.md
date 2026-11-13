# Batch Report

**Batch**: 57
**Tasks**: AZ-4267_device_grant_lookup_binding (done, security High), AZ-4268_token_type_prefixes (done), AZ-3735_transfer_put_block (done), AZ-3842_derived_store_accept_input (done)
**Date**: 2026-11-13

Three parallel workers (`01_identity` + `sc_ids` + 06's token code for the two security fixes; `03_transfer`; `08_media_jobs`, later with the `RevisionView` contract field and 02's revision mapping). The parent wrote the device-grant migration, added one test dependency (03 `futures-core`), approved one additive contract field (`RevisionView.blob_id`), made two follow-ups itself (11 reads the device-token prefix from `sc_ids`; sign-in one-time values use a new unprefixed generator), wrote every document change, reviewed, ran the gate and committed.

## Owner decisions 2026-11-13

1. AZ-4267: a pending device grant is bound at lookup to the account that looked it up (migration `20261113110000`, `device_grants.looked_up_by_account_id`).
2. AZ-4268: every token kind carries a type prefix (`sc_ses_`, `sc_ref_`, `sc_dev_`, `sc_dcd_`, `sc_inv_`, `sc_set_`, `sc_shr_`, `sc_lgc_`).
3. AZ-3735 and AZ-3842 as specified; block upload checks a deleting account through `AccountGuard` in the checkpoint transaction (every 8 MiB or 1 s).

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-4267_device_grant_lookup_binding | Done | `lookup_for_approval` binds the pending grant with one conditional UPDATE (`looked_up_by_account_id IS NULL OR = caller`), so of two concurrent lookups by two accounts exactly one binds; `decide` (and the expiry and existence statements) filter by the binding: another account, a never-looked-up grant or an unknown id all answer `UnknownCode` with no change and no audit. The AZ-3692 defect test runs again and passes; AC-1..AC-4 |
| AZ-4268_token_type_prefixes | Done | `TokenKind::prefix()` (const), `new_token` = prefix + 43 base64 characters, `RawToken::is_of_its_kind()`, `sha256_token` over the full text. Each single-kind hash store refuses another kind's token by hash alone; the one mixed store (`invites`: invite and setup) is guarded at sign-in begin (`invalid_request` for a prefix that is not the field's kind). The share service uses the helper's prefix. The AZ-3692 defect test runs again and passes |
| AZ-3735_transfer_put_block | Done | `put_block`: in-memory session, index, expected length, duplicate rules, lane, account and buffer caps — all before reading the body; length and BLAKE3 before `write_block`; the bit after `await_durable`; a checkpoint transaction every 8 MiB or 1 s with `AccountGuard` (owner), never touching a committed or expired row. No database call per block. AC-1..AC-7 on the real 04 writer (four racing lanes over 75 MiB with 10% duplicates assemble the exact file) |
| AZ-3842_derived_store_accept_input | Done | `accept_output`: holder check (running under a live lease, or already closed `derived` by the same worker so a lost answer retries to `AlreadyPresent`), kind, size cap and dimension bound, revision check, write with no transaction open, type sniffed from the stored head, one transaction (`register_in_tx`, `derived` row, surplus file queued now, job closed when every output exists). 8 racing posts store one file; a GC cycle removes the surplus. `open_input` streams the decrypted original of the job's (current or retained) revision to the lease holder only |

## Parent changes

- Migration `20261113110000_add_device_grant_lookup_binding` (up, down, up checked on a copy of the test template).
- 11: `CredentialFormat.device_token_prefix` removed; a bearer is a device token when it carries `TokenKind::DeviceToken.prefix()`; tests use `sc_dev_` tokens.
- `sc_ids::new_random_text()` (no prefix) for the sign-in state, nonce, PKCE verifier and browser binding (they had started to carry `sc_ses_`); test of 1,000 draws.
- 03 `futures-core` dev-dependency (lock: one edge).
- `contracts::RevisionView.blob_id` (approved; with `#[allow(clippy::large_enum_variant)]` on `LiveFileLookup`, as `media-shared` does).

## Contract and document changes (parent)

`sc_ids.md` 1.2.0 (prefixes; open point 1 closed); `data_model.md` §1 (secrets at rest) and §4.5 (`looked_up_by_account_id`); part 1 1.4.0 (device grant binding); 01 description §5; `upload_service.md` 1.4.0 (`put_block`, restart gap, defaults); AZ-3736 spec (lazy load needs a key port; reaper and abort remove progress entries); 08 description (DerivedStore as built, dimension default, `RevisionObsolete` vs `LeaseLost`); `http_server.md` (no device-prefix setting).

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): fmt, workspace clippy `-D warnings`, workspace tests on PostgreSQL 18 (3324 passed, 0 failed, 21 ignored — the two AZ-3692 defect tests run again), wasm build and the four check scripts all exit 0.

Accepted deviations: another account's decide answers `UnknownCode` (the contract's answer for an unknown id, same 404 on the wire) rather than a new `NotFound`; the wrong-kind token check relies on single-kind hash stores plus the begin check, not on a fallible `from_presented` (which would have changed 11's answer to a malformed body).

Warnings (recorded):
- After a restart `put_block` cannot rebuild a session's write handle: `BlobCatalog` does not serve an `uploading` blob's key (AZ-3736 needs a port).
- A deleting account's last block before the checkpoint is already durable when `AccountNotActive` is answered.
- `RevisionObsolete` is nearly unreachable for 08 (the `jobs` FK cascades on revision delete): spec wording for IT-08-11e, IT-08-12 and AC-72 should say a collected revision's job is gone (`LeaseLost`).
- A short output body answers `OutputTooLarge` (04's `SizeMismatch` has no direction); a dimension violation answers `OutputTypeRejected` (no specific code).
- `derived_bytes_total{kind}` not added.
- At sign-in begin an invite/setup token of the wrong kind is `invalid_request`, while an unknown invite is refused later as `InvitationRequired` (matching it would need the kind in `oauth_states`).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | A same-hash duplicate of an in-flight block is `Overloaded` (retry); a repeat of an acked block is acked without reading its body | AZ-3735 | Recorded in `upload_service.md` | unclear |
| 2 | Account B deciding A's already decided grants now gets `UnknownCode`, not `AlreadyDecided` (B learns nothing) | AZ-4267 | Test expectation updated | clear |
| 3 | `open_input` read sizes are 4 MB and 12 MiB, not DS-03/DS-07's 150 MB (no fixture in the repo) | AZ-3842 | As implemented | clear |
| 4 | The device-token marker used to be a required setting with no default | AZ-4268 | Removed; derived from `sc_ids` | clear |

## Commit

`[AZ-4267] [AZ-4268] [AZ-3735] [AZ-3842] Device grant binding, token prefixes, put_block, derived store` — Loop: 1

## Next Batch

Recompute the ready tasks; cumulative review of batches 55-57 is due.
