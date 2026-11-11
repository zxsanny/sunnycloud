# Batch Report

**Batch**: 41
**Tasks**: seven decision and research spikes done (AZ-3787, AZ-3867, AZ-3887, AZ-3913, AZ-3933, AZ-3973, AZ-3986), one partly done (AZ-4004), seventeen blocked on hardware or access I do not have. Documents only, no code.
**Date**: 2026-11-11

Four of the seven were run by parallel workers on disjoint files (AZ-3787, AZ-3867, AZ-3933, AZ-3986); the parent applied `tests/spec-gaps.md`, checked the diffs and kept tracker, commit and push.

## Task Results

| Task | Status | Decision | Owner or agent |
|------|--------|----------|----------------|
| AZ-3787_migration_test_source_spike | Done | Test-only migration source is the cargo feature `test-migrations` of the test image (`FEATURES=test-clock,test-migrations`); `SUNNYCLOUD_TEST_MIGRATIONS_DIR` merges a fixtures directory with the embedded migrations in `migrate` and `serve`'s check; a release build refuses the variable (exit 3), asserted in CI on the pushed digest. IT-12-11 runnable | agent |
| AZ-3867_worker_spike_slots_clock_scratch | Done | Slot boundaries listed (one job per slot, lane mask, own heartbeat, own socket and scratch); the offset clock affects only `image_age_days`; interactive scratch is 512 MiB in both docs (arithmetic: 256 + 2 x 32 + 192 margin); `containerization.md` changed from `1g`; scratch-sizing and secret-rotation runbook section added | agent |
| AZ-3887_update_manifest_adr_spike | Done | Layout confirmed as written in ADR-017; offline signing tool is a second binary (`update-sign`) of the updater crate that reads the key only from a 0600 file and refuses on CI; runbook step and the ten DS-31 fixture recipes written; prototype proved bytes-as-signed (exact bytes verify; one flipped byte or a whitespace-only change fails) | owner (layout), agent (tool) |
| AZ-3913_vault_decisions_spike | Done | One vault per account; encrypted names (owner). Membership is a nullable `nodes.vault_id` copied from the parent at creation; a move across the vault boundary is 409 `vault_boundary`; the vault root must be empty at creation; `max_vault_header_bytes` defaults to 1024 | owner (two), agent (rest) |
| AZ-3933_setup_page_spec_spike | Done with open points | First-run page drafted on route `/setup` from the mockup, token sent only in the login-attempt body, checked late (after the provider round trip); IT-16-12 rewritten; seven open points listed in 16 description section 5 | agent draft |
| AZ-3973_ios_ac78_sibling_spike | Done | AC-81 [v1.1 iOS] added; IT-18-9 traced to it; shared mutex and key rules hold on iOS, the only difference is the trigger layer | owner |
| AZ-3986_play_declarations_spike | Done with unverified items | Verdicts with sources in 17 description section 16 (new): media permissions allowed with a declaration, approval unverified; direct battery-exemption request treated as refused (settings screen only); user-initiated data-transfer job allowed; an always-on foreground service is refused | agent research |
| AZ-4004_vault_review_plan_spike | **Partly done, stays in `todo/`** | Draft plan and fallback rule written in 19 description section 11; the reviewer, budget, schedule and the owner's sign-off are not decided | owner needed |

## Blocked (not started; nothing substituted)

| Task | What is missing |
|---|---|
| AZ-3799 (Caddy HTTP/2 goodput) | 1 GbE link at 20 ms RTT between two hosts; `tc` needs root |
| AZ-3849, AZ-3850, AZ-3851 (decoder) | `vipsthumbnail` is not installed (needs apt), the phone clips and red-team set, and timings only count on REF-CPU4 |
| AZ-3881 (100k files) | WIN-VM and the Linux reference machine; this host could give only an informational Linux half |
| AZ-3882 (settings cold start) | REF-WIN-WEAK and REF-LIN-WAYLAND-NV |
| AZ-3883 (Windows logon task) | a Windows machine |
| AZ-3884 (file dialogs) | Windows, GNOME with and without portal, wlroots without portal |
| AZ-3885 (keyring under systemd) | no Secret Service daemon here (`gnome-keyring-daemon` and `secret-tool` absent) and no way to test headless and locked contexts |
| AZ-3886 (screen readers) | NVDA, Narrator and Orca, manual |
| AZ-3921 (macOS placeholders, TCC) | a Mac |
| AZ-3930, AZ-3934 (web WASM, too many files) | Firefox and WebKit under Playwright (only Chromium is installed) |
| AZ-3985, AZ-3987, AZ-3988 (Android) | emulator plus phones (Pixel, Samsung, Xiaomi) |
| AZ-4005 (Argon2 on devices) | a low-end Android phone, iOS and Mac Safari |
| AZ-3893 | Windows only |

## Code Review Verdict: PASS

Documents only. Seven specs archived to `done/`; Jira `In Testing` for the seven.

## Test Suite

Not run: no code changed (last full run in batch 37: 2762 passed, 0 failed, 9 ignored).

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | First-run page: accept route `/setup`? does `/s/<token>` redirect? trim the pasted token, empty "Continue"? accept that a wrong token shows only after the provider round trip, or add an earlier check in 01? | AZ-3933 | Owner answers needed; seven open points in 16 description | unclear |
| 2 | The CI check for the test-migrations feature relies on config binding rejecting an unknown `SUNNYCLOUD_*` key before any DB access, even for `migrate status` | AZ-3787 | E-01 and E-22 must keep that order | unclear |
| 3 | Killed-migration test needs the harness to `pg_terminate_backend` the orphaned backend | AZ-3787 | Written as a harness fault-arrangement step | clear |
| 4 | Follow-up work from AZ-3787: `sc_db` feature and merging `Migrator` constructor (E-06/c01b), the `test_migrations_dir` key (E-01), test image `FEATURES`, read-only fixtures mount, release-digest check (E-00) and `tests/environment.md` mention | AZ-3787 | Listed in 12 description section 4; `environment.md` not edited | unclear |
| 5 | Secret rotation steps assume `worker_api_token_next` is a compose secret file; no doc declares it in the compose secrets table | AZ-3867 | The new runbook says to declare and later remove it; owner to confirm the mechanism | unclear |
| 6 | The vault header gains an encrypted name field; `vault_format.md` open point 7 asks E-29/c01 to freeze it with the reviewer; the 1024-byte cap is my proposal | AZ-3913 | Owner may overturn; raise the default if the frozen header is longer | unclear |
| 7 | Vault membership edge cases (move across the boundary refused, empty root at creation) and the case-stable name encoding are agent proposals | AZ-3913 | Documented in 20 description section 10 | unclear |
| 8 | Play: approval of the photo and video permissions is unverified and the one item that can fail late; the fallback is a Photo Picker-only Play build, which changes E-27 tasks c09, c10, c11, c15 and breaks AC-9 and AC-53 (owner decision if refused). The Play account, closed testing track and declarations do not exist yet | AZ-3986 | Owner items listed in 17 description section 16 | unclear |
| 9 | The fallback rule for the vault stream (libsodium `secretstream`) and the review plan scope are drafts; reviewer, budget and schedule are the owner's | AZ-4004 | Task stays in `todo/` | unclear |
| 10 | Items from batches 29-40 stay open (blob-encryption review reviewer and date; remaining 10 MiB figures; batch 38 items 1-3) | earlier batches | | unclear |

## Commit

`[AZ-3787] ... [AZ-3986] Decision and research spikes` — Loop: 1

## Next Batch

Ready with a plan: AZ-4004 (needs the owner's reviewer, budget and schedule). Everything else ready is blocked in the table above. 238 tasks left.
