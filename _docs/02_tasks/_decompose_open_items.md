# Decompose open items (greenfield Step 6, closed 2026-11-05)

**Result:** 367 tasks, 1,255 points, 31 epics (E-00..E-30; E-31 blackbox tests are decomposed at greenfield Step 9), every task ticketed in Jira (project AZ, label `sunnycloud`), dependencies resolved to ticket names, no cycles, execution order in `_dependencies_table.md`. Every interaction risk R1-R13 has an owning task. Each open item below is also a `## Flagged concerns` row (status `open`) in the task that hits it; none is Critical.

## Owner decisions of 2026-11-04 / 2026-11-05 (applied)

| Decision | Applied in |
|----------|-----------|
| Layout, names (`media-api`, `sunnycloud-server`/`sunnycloud`, `sync-core-proto`, `sunnycloud-daemon`) approved | `module-layout.md`, AZ-3652 |
| E-06 writes the base `accounts` table; E-15 extends it | AZ-3783, E-15 tasks |
| Decompose everything, v1.1 and v1.2 included | all epics |
| New web vault page (v1.2) | AZ-3954 |
| G1-G4 gap tasks added | AZ-4021 (CSP / release Caddyfile), AZ-4022 and AZ-4023 (doctor checks), AZ-4024 (Sign in with Apple, v1.1), AZ-4025, AZ-4026, AZ-4027 (mobile vault, v1.2) |
| H1 health traits live in `sc_observability`; `contracts` re-exports | AZ-3786, AZ-3768, contracts docs |
| H2 account-erase hook order: 06, 20, 02, 03, 08, then 05, 07a | AZ-3925, AZ-3820, AZ-3665, AZ-3789, AZ-3922 |
| H3 approval lookup returns grant id and expiry | AZ-3687, AZ-3944, contracts part 1 |
| H4 no vault share links in v1.2 | 16 §5, F17, NFT-SEC-42 step 5, AZ-3954 |

## Still open after H1-H4

- H1 is half fixed: `sc_observability` still depends on `sc_db` (AZ-3783), so sqlx still lands in the worker and decoder images. Needs a split of the observability crate or a feature flag (flagged High on AZ-3868 and AZ-3853).
- H5 external vault review has no reviewer, scope or budget (owner action before v1.2).
- AZ-4026 needs a sync-core vault operation (`sync_api.md` has none; unclear whether upload and batch commit carry `client_meta`): may need an E-23 task first.

## Stale text to fix at greenfield Step 13 (Update Docs)

- H2: `data_model.md` §12, E-17 epic lines 24 and 128, ADR-015 line 54, 07 description line 29, AZ-3922 header text.
- H3: 01 description line 47 (`ApprovalView`).
- H4: `diagrams/flows/flow_vault.md`, NFT-SEC-42 title and Data cell, `interaction-risks.md` line 25, AZ-3913 line 101.
- README and `problem.md` still name Facebook sign-in (out of scope per `restrictions.md`).
- FT-N-49 (obsolete vs retained revisions): decided 2026-11-07 (obsolete only when collected or foreign), FT-N-49 reworded; `tests/spec-gaps.md` G-AS-1, G-AS-2.

## Gaps with no owning task (need new tasks)

| # | Gap | Release | Proposed owner |
|---|-----|---------|----------------|
| G1 | Nothing emits the SPA's CSP and security headers (Caddyfile has none; IT-16-9 cannot pass) | v1.0 | E-00 (Caddy/compose) or E-21 |
| G2 | Owner `DoctorCheck`s of 01 (providers, redirect URIs, setup state) and 11 (public URL, TLS, HTTP/2, proxy probe) | v1.0 | E-15 and E-21 |
| G3 | Server-side Sign in with Apple provider (AC-19; iOS depends on it) | v1.1 | E-15 |
| G4 | Mobile vault: vault-core in the mobile libraries and a mobile vault screen (the UniFFI surface has no caller) | v1.2 | E-27 / E-28 |

## High design points (recommended fix in brackets)

| # | Problem | Recommendation |
|---|---------|----------------|
| H1 | `sc_observability` (E-09, AZ-3786) imports `contracts`, which pulls `sc_db`/sqlx into the worker and decoder images | Move `HealthContributor`/`ReadinessCheck` into `sc_observability`; `contracts` re-exports them |
| H2 | `vaults.root_node_id` FK breaks account erase (02's hook runs before 20's) | Run 20's erase hook before 02's |
| H3 | `ApprovalView` has no grant id or expiry, so the approval page cannot approve/deny or show the countdown | Add both fields to the 01 approval lookup (E-15/c07 AZ-3687, contracts part 1) |
| H4 | Vault share links: 16 §5, F17 and NFT-SEC-42 step 5 say key-in-fragment links; canonical names, 19, 06 and AZ-3675 say none in v1.2 | Keep none in v1.2; fix the three docs |
| H5 | External vault review has no reviewer, scope or budget | Owner action before v1.2 |

## Owner decisions already open in specs (Medium)

G-03-1 resend bound (16 vs 10 MiB, AC-3) · batch too large 413 vs 422 (decided 2026-11-07: 422; body over the `Batch` class 413) · G-11-1 409 vs 412 (decided 2026-11-07: 409) · request id fresh vs kept · G-AS-1 delete another user · G-AS-2 bandwidth unit · FT-N-49 obsolete vs retained revisions (decided 2026-11-07: obsolete only when collected or foreign) · mobile sign-out route (`DELETE /devices/me` is device-only) · DeviceOnly scope class · vault build exclusion for v1.0/v1.1 (ADR-016).

---

# Raw notes by batch


## Mechanical
- Resolve every `E-NN/cKK` / `E-NN/pKK` ref in `**Dependencies**` to `AZ-<key>_<short>` via `_docs/02_tasks/_ref_map.tsv`.
- Alias: `E-06/c01` → both `E-06/c01a` and `E-06/c01b` (B01 split). AZ-3654 (first-run pipeline) needs `E-06/c01b`.
- `E-31/cKK` refs (blackbox test epic, decomposed at Step 9): keep as plain-text "E-31/cKK (test epic, Step 9)" — not a task yet.
- AZ-3652 Implementation Order: `contracts` parts 1-3 come after E-02, E-03, E-04 (B01 finding); fix the table.
- E-30/p01 (AZ-3675 vault pipeline): add dependency on the new E-26 web vault page task (B12).

## Owner-level concerns to surface (collect; present once at Step 4 BLOCK)
- B03: G-03-1 (16 MiB resend vs AC-3 10 MiB); G-03-3 (stale-base commit answer); AccountGuard missing from 03 Ports/module-layout; job key needs account id (PublishedEntry); Updated vs Replaced undefined; revive-before-cascade; restore-by-path ambiguity; batch-too-large 413 (F5/FT-N-48/AZ-3656) vs 422 (03); `sync_conflict_copies_total` unowned.
- B02: crate-graph rule vs component dev-dependencies in tests; epoch doctor check source; doctor/health levels; ST-04-8 block rewrite rule + external encryption review owner; session-cap default G-R9; config key `default_quota` vs `default_quota_bytes`; REF-SRV not provisioned.
- B04: DeviceOnly scope class (11) vs 4 scopes (01); losing concurrent refreshes outcome; allowlist at invite redemption; identity invalid-token limit number + mandatory audit events; Argon2id defaults; role value member vs user.
- B01: worker job DTOs declared twice (contracts vs media-shared); init-script role passwords secrets; NOT-RUN code for "not built yet"; environment.md vs canonical-names service/network names; `authorize_derived` missing in 06; error codes missing in helper 02; E-10/c01 overlap with E-00/c05; doc spelling fixes (data_model §1 keys, correlation_id vs request_id).
- 1.7: E-25/c14 settings suite should run against the real daemon.
- Alias: `E-22/c14` → `E-22/c14a` + `E-22/c14b` (B08 split). AZ-3666 references E-22/c14.
- B08: E-22/c17 (acceptance) depends on pipelines AZ-3654/AZ-3666 — allowed (test task downstream of pipeline).
- B08 owner concerns: vault crates out of v1.0/v1.1 builds (mechanism); G-12-1; no contracts port for shutdown checkpoint flush/final group sync; `admin create --note --ttl` vs create_admin_invite(); AccountDeletionService::request needs a principal for operator; TreeRestore dry-run; --new-key-file vs rewrap_batch; Scrubber per-blob findings; describe_blobs lacks ids; owner DoctorChecks of 01 and 11 have no task (E-15/E-21); shutdown grace 120 s vs 25 s.
- B07 owner concerns: G-11-1 409 vs 412 (decided 2026-11-07: 409 for both); batch too large 413 (F5/FT-N-48/AZ-3656) vs 422 (helper 02, 03) (decided 2026-11-07: 422; a body over the `Batch` class is a separate 413; F5, FT-N-48, AZ-3656 changed); DeviceOnly class; request id fresh vs kept (11/IT-11-4 vs sc_error/sc_observability/16 ApiClient); session vs device token prefixes (sc_ids open point 1); no config for update_url and public :8080 key; revisions:lookup body class (~70 KB > Small); thumbnail-states node ids vs content hashes (G-08-3: foreign and unknown ids omitted, over 1,000 ids 422, decided 2026-11-07); RFC 8628 poll status codes; :9091 /test/clock auth; GET provider callback mutates (exemption).
- B05: FT-N-49 (Step 5 scenario) expected `obsolete` for superseded/deleted revisions while 08 §4/IT-08-12/tree contract serve retained revisions -> decided 2026-11-07: obsolete only when collected or foreign; FT-N-49, F10 and data model §9.1 reworded. Other B05 owner concerns: purger claim / operator deletion path / Forbidden on AdminService need a contracts change task; account-deletion confirmation value unspecified; no port for AccountView display name + identities; DevSeed account without provider sign-in; wire codes for ConfirmationMismatch, InvalidLimit, output 413/422; PublishedEntry lacks account_id.
- B05 contract consumers in other batches should list 07/08 contracts as document deps (E-20, E-21, E-22, E-13, E-14, E-10, hook implementers) — add at Step 4 where missing.
- Aliases (B06): `E-19/c07` → c07a + c07b; `E-19/c14` → c14a + c14b; `E-20/c10` → c10a (v1.0; c10b is v1.1). AZ-3661: its open concern about the supervisor main now points to E-20/c08 (main composition added there).
- B06 HIGH: E-09 sc_observability imports `contracts` → sc_db/sqlx would land in worker + decoder images (against module-layout / no-sqlx check). E-09 needs a split or a feature without that edge. Fix at Step 4 (edit E-09 task AZ-3786 + module-layout) or ask owner.
- B06 other: params_version missing in render_still/Plan; decoder media build + weekly rebuild/release workflow ownership (E-19/c14a); seccomp profile referenced from compose files (E-00/c11, E-31/c05, release bundle); sc_clock offset only grows vs per-test reset; E-20/c11 suite depends on real API (vs fakes-first) — confirm.
- Aliases (B10): `E-24/c10` → c10a + c10b (AZ-3669 p02 depends on it); `E-24/c12` → c12a + c12b (AZ-3670 p03). E-23 split c09 → c09a + c09b (B09).
- B10 owner concerns: per-file errors (FT-N-51 rejected names, mockup Errors list, too large) have no field in DaemonStatus/IPC; bandwidth limit has no enforcer in 13 + unit open (G-AS-2); RestoreFromServer after 200+ remote deletes + confirm path for intentionally emptied root; staged update apply (deb/MSI); "user active" detection for hash budget; offline signing tool owner (proposed c18); Windows logon task elevation (AC-26); `%5F` marker + ` (2)` suffix (ST-14-5); token file location G-12; LoginPrompt.expires_at source; daemon logging owner (suggest p01 AZ-3668).
- B11 HIGH: `vaults.root_node_id` FK vs erase order (02 hook before 20) → AC-27 erase breaks; needs delete rule or hook reorder. Other: E-30/c07 edits 02/03/06 dirs (maybe split into E-13/E-14/E-16 tasks); vault build exclusion mechanism (ADR-016); ipc_schema gaps for the window (pause all carrier, Errors list, AccountView names, PairView server path, validate-only rules method, endpoint path rule + max frame, start command); doc fixes (F17/NFT-SEC-42 vault share links, matrix AC-60 'parent folder', 15 tests IT-15-14 BLOCKED, 20 tests AC-61 mapping, 20 best-effort vs sc_audit).
- Aliases (B12): `E-26/c07` → c07a (AZ-3654 first-run uses c07a; AZ-3694 mention → c07b); `E-26/c19` → c19a + c19b (AZ-3671). New tasks E-26/n01 (web vault page v1.2) and E-26/n02 (design tokens/components). AZ-3675 (vault pipeline) must depend on E-26/n01.
- B12 HIGH: (1) vault share links: my B12 brief, 16 §5 and NFT-SEC-42 step 5 say key-in-fragment share links; canonical-names, 19, 06, AZ-3675 AC-2 say no vault share links in v1.2 → owner decides; fix 16 §5 / NFT-SEC-42 / F17 accordingly. (2) No task emits the CSP header (Caddyfile none, API serves SPA without) → IT-16-9 cannot pass; needs an owner task (E-00/c11 compose/Caddy or E-21). (3) ApprovalView lacks grant id + expiry → approve/deny call + countdown impossible (E-15/c07, E-00/c03 contracts).
- B12 Medium: no API for share default/max lifetime; invite link format; sign-in error route/codes; listing sort + folders-first unsupported by API; thumbnail-states body (G-08-3, decided 2026-11-07); S12 unproven; vault membership marker/one-or-many vaults/no vault mockup. Low: passphrase zxcvbn 3 (16 §5) vs 4 (FT-N-37).
- Aliases (B09): `E-23/c09` → c09a + c09b (AZ-3669, AZ-3673); `E-23/c18` → c18a + c18b; `E-28/c11` → c11a + c11b (AZ-3674).
- B09 owner concerns: ApiError::parse drops unknown codes (IT-13-13 needs raw code; E-02); EditedSince{current} has no revision id field; sign_out uses DELETE /devices/me (device-only) → mobile sign-out needs a route (E-15/E-21/E-27); reference hardware unnamed (S1, PT-13-1/4); E-28 needs E-27 changes (AssetSource group_id/content_identifier/NeedsDownload, iOS checkpoint columns) — no E-27 task; E-28 Flutter edits 17-owned files; NO TASK adds server-side Sign in with Apple (v1.1, AC-19) — check E-15; AGPL App Store exception consent + COPYING.iOS. Low: IT-13-16(B) blocked on E-14/c02; WASM commit-id export shape; revisions:lookup body class; longpoll_max_wait unit; planner action for exhausted block retries.
- B13: add `E-27/c06b` (iOS shared-crate additions, v1.1) as dependency of AZ-3976 (E-28/c04), AZ-3981 (E-28/c09), AZ-3982 (E-28/c10); module-layout 18 entry ("read-only for 18") must allow 18's iOS screens in the E-27/c15 Flutter shell.
- B13 owner concerns: external vault review reviewer/scope/budget (High); AZ-3739 AC-3 allows wasm-bindgen only in sync-core-wasm but vault-core needs it (allowlist or adapter crate); no task assembles vault-core into mobile libs / no mobile vault UI (UniFFI surface uncalled); BackupStatus lacks mockup fields (last backup time, time left, account name, storage used/total); mobile sign_out route (DeviceOnly); assets leaving visible set + failed rule; edited-photo name collision; FreeSpaceFloor PauseReason; vault passphrase normalisation/recovery checksum/chunk-size placement; 17 session table vs 13 UploadCheckpoint; client_meta bound (spike).
