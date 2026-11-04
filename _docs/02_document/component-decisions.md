# Component List and Boundary Decisions

**Status**: confirmed-by-user
**Date**: 2026-11-04

Single source of truth for the reconciliation pass over the component specs. Where a spec, `architecture.md`, `data_model.md` or a deployment doc disagrees with this file, this file wins.

## Final component list

| # | Component | Note |
|---|-----------|------|
| 01 | identity | sign-in, sessions, device grant, invites |
| 02 | tree | files, folders, revisions, names, conflicts, commit transaction |
| 03 | transfer | upload sessions, blocklists, quota reservation, download |
| 04 | blobstore | layout, encryption, GC, scrub, restore |
| 05 | sync_feed | change feed, long-poll, resync |
| 06 | sharing | share links (files only in v1.0) |
| 07 | admin | users, invites, account deletion (split from the old `07_admin_quota`) |
| 07a | quota | quota ledger (new split) |
| 08 | media_jobs | job queue, derived files |
| 09 | worker_supervisor | claims jobs, drives decoders, applies the transcode policy |
| 10 | decoder | sandboxed vips and ffmpeg; probes only |
| 11 | http_api | delivery layer |
| 12 | operator_cli | the `sunnycloud` binary: composition root, janitor loops, scheduler |
| 13 | sync_core | sans-IO protocol part (also builds to WASM for the web app) plus the client part |
| 14 | desktop_daemon | with its own IPC schema crate |
| 15 | desktop_settings | |
| 16 | web_app | |
| 17 | android_app | with a `mobile_backup` crate |
| 18 | ios_app | v1.1 stub |
| 19 | vault_core | v1.2 |
| 20 | vault_service | v1.2 stub: server side of the vault |

Helpers: 01 config, 02 error, 03 clock, 04 audit, 05 ids_hashing, 06 names, **07 db (new)**, **08 ratelimit (new)**.

Shared crates: `contracts` (port traits; breaks dependency cycles between components) and `media-shared` (media types, queue SQL, decoder protocol).

## Decisions

1. **AC-53:** Android 14 limited access hides the library total. The app shows "N photos visible, tap to allow more" and drops the "of M".
2. **Video with the index at the end:** when codecs are already compliant, lossless remux (copy streams, move the index to the front) instead of a re-encode.
3. **Compliance bound:** the 1920x1080 limit is orientation independent (long side <= 1920, short side <= 1080). Poster frame at 1 s, or 10% of the duration for clips shorter than 10 s.
4. **Restore into a deleted folder:** the file goes to its original folder if it exists, otherwise into a "Restored" folder at the root.
5. **Missing epoch bump:** `doctor` detects it from an `.epoch` marker on the blob volume.
6. **Scrub summaries:** stored as audit events and metrics; no new table.
7. **Mobile scope:** file browsing and download on Android are out of scope for v1.0.
8. **Updater:** the daemon verifies an Ed25519-signed update manifest hosted on GitHub Releases; the source is fixed at build time.
9. **Escaped names:** `%XX` escaping for names a client cannot represent.
10. **Test clock:** one test-image endpoint `GET /internal/v1/test/clock-offset` so the supervisor and API share one offset (replaces `POST /test/clock` where the supervisor needs the time).
11. **Data-model additions:** `login_codes` table; `revisions.client_meta`; staged small uploads as a completed upload-session row with a staged-file cap; optional `blobs.unavailable_at`; job outcome `obsolete` plus an index on queued jobs.
12. **v1.0 runs one API process:** upload progress is in memory, so replicas are unsupported for uploads.
13. **Deployment fixes:** Caddy proxies to the API over h2c; workers get a second API listener on `:8081`; two decoder containers per supervisor (interactive and general; the general one has disk-backed scratch); database roles are `sunnycloud_migrator` and `sunnycloud` (superseded at Step 4: there is no worker role; see `canonical-names.md`).
14. **Transcode policy** is applied by the supervisor from `media-shared`; the decoder only probes.
15. **Settings and sharing:** share links target files only in v1.0; the last admin cannot be removed; two new settings (a share grant key and a maximum share lifetime); the device label in conflict-copy names is the committing device.
16. **Janitor loops and the composition root** both live in the `sunnycloud` binary.
17. **Operator commands:** add `keycheck-init`, `rewrap`, `media-requeue`.

## Known drift to fix in the pass

- CLI command names differ between components, `architecture.md` and deployment docs.
- Role names `sc_*` versus `sunnycloud` / `sunnycloud_worker`.
- Caddyfile HTTP/1.1 versus h2c.
- Missing second decoder and the `:8081` listener in containerization and `architecture.md`.
- `interaction-risks.md` owner of AC-78 (Android app owns it, via the `mobile_backup` crate).
- Dependency cycles between components: every cross-component call goes through a `contracts` port trait; transaction ownership: the commit transaction belongs to 02 tree, which calls the 07a quota port and the 05 sync_feed port inside it.
