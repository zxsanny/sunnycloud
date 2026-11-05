# Contract: DaemonControl behaviour

**Component**: 14 desktop_daemon (daemon binary; trait and types in `ipc_schema`)
**Producer task**: E-24/c19 — `_docs/02_tasks/todo/B10-21_daemon_control_impl.md`
**Consumer tasks**: this batch: E-24/c16 (dispatch), E-24/c17 (tray menu); other batches: E-25 (the stub daemon must answer the same way; screens; UI suite against the real daemon), E-24/p01..p03, E-31 (desktop scenarios)
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

`ipc_schema.md` fixes the names and types. This contract fixes what each `DaemonControl` method does: its effect, when it fails, how fast it answers and which status change it causes. 15 and the tray rely on this behaviour, and E-25's stub daemon must imitate it, so it is shared.

Sources: `components/14_desktop_daemon/description.md` §2, §5; `components/14_desktop_daemon/tests.md` IT-14-15, PT-14-4; `components/15_desktop_settings/description.md` §1, §5; AC-16, AC-48, AC-54, AC-66.

## Shape

| Method | Effect | Fails with |
|---|---|---|
| `status` | Returns the current `DaemonStatus`. Never contacts the server. | never |
| `set_server` | Checks the URL with `ServerUrl` (https only, unless the developer build feature allows http), probes `GET /api/v1/server-info` through 13, stores the address in `meta`. | `HttpsRequired` (not https), `ServerUnreachable` (probe failed), `UpgradeRequired{message}` (server's minimum client version above this build), `SignedIn` (a change while signed in) |
| `list_server_folders` | Lists the folders (not files) under `parent`, or under the account root when `parent` is absent, one page per call. | `NotSignedIn`, `ServerUnreachable`, `UnknownFolder` (node unknown or not a folder) |
| `list_pairs` | Returns every pair with its state. | `NotSignedIn` |
| `add_pair` | Validates the spec, creates the server folder path when `create_if_missing` is set, probes the root for case-insensitivity and stores the result, stores the pair `active`; a non-empty root or server folder starts the first-sync merge. | `RootInvalid` (local root missing, not a folder or not writable), `RootNested` (inside or around another pair's root), `AlreadyPaired` (server folder already paired), `UnknownFolder`, `CloudManagedRoot` (root inside a detected cloud-managed folder), `RulesInvalid{line}`, `NotSignedIn` |
| `update_pair` | Replaces the pair's rules; the new rules govern every later hint and feed row within 5 s (AC-66, AC-16). On `RulesInvalid` nothing changes. | `UnknownPair`, `RulesInvalid{line}` |
| `remove_pair` | Stops the pair's worker and forgets the pair and its entries. | `UnknownPair` |
| `pause` / `resume` | `pause` stops uploads and feed applies for the pair (state `paused`); `resume` returns it to `active`. | `UnknownPair` |
| `list_skipped` / `list_escaped` | Pages over the entries skipped (placeholders, symlinks, junctions, special files) or saved under an escaped name, for one pair or all. | `UnknownPair` |
| `get_settings` / `set_settings` | `set_settings` validates every key of the patch, applies the valid patch at once, persists it in `meta` and returns the applied values (echo). A patch with any invalid key changes nothing. | `SettingInvalid{key}` |
| `resolve_hold` | `ConfirmDeletes` sends the held deletes (with base revisions); `RestoreFromServer` re-downloads the held files; the pair returns to `active`. | `NoSuchHold` (pair not in a hold) |
| `login_start` | Starts the RFC 8628 grant through 13, opens the system browser on the verification URL, starts polling. Returns URL, user code and expiry; never a token. | `ServerNotSet`, `ServerUnreachable`, `HttpsRequired` |
| `login_status` | Returns the login state; `SignedOut` after a revoked token or a logout. | never |
| `logout` | Calls `SyncApi::sign_out` (`DELETE /api/v1/devices/me`), then deletes the token from the token store even when the server call failed; `server_revoked = false` reports that the server copy still needs revoking. | never |
| `subscribe` | Registers the connection; from then on every change of any `DaemonStatus` field sends one `StatusEvent` with the full new status. | never |

## Invariants

- No method returns, logs or echoes a token, secret or user code beyond `LoginPrompt.user_code` (AC-48; logging rules 14 §9).
- Every method answers within 5 s (15's request timeout). `set_settings` answers within 200 ms at p95 and its value is effective within 1 s (failure above 1 s response or 5 s effect; PT-14-4, AC-16).
- No method deletes a file, locally or on the server, outside the delete guard's rules (14 §5: nothing is deleted except via the guard); `remove_pair` deletes neither local nor server files.
- A state change caused by a method (pause, resume, hold resolved, pair added or removed, settings) emits its `StatusEvent` before or together with the method's response.
- Methods apply immediately; there is no "save" step and no restart.

## Non-Goals

- Not covered: the wire format and the endpoint (`ipc_schema.md`); the order in which `add_pair` checks its errors when several apply; the settings window's messages.

## Versioning Rules

- **Breaking**: an effect removed or changed (for example `logout` no longer deleting the local token), a tighter precondition, a slower budget.
- **Non-breaking**: a new error case that maps to an existing variant, a faster budget, a new status field set by an existing effect.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| http-address | `set_server("http://x")` | `HttpsRequired` | IT-14-15, AC-54 |
| server-too-new | server with `min_client_version` above the client | `UpgradeRequired{message}` before sign-in | IT-14-15 |
| change-while-signed-in | `set_server` while signed in | `SignedIn` | IT-14-15 |
| nested-root | `add_pair` with a root inside an existing pair's root | `RootNested` | IT-14-15 |
| same-folder-twice | `add_pair` twice for one server folder | `AlreadyPaired` | IT-14-15 |
| bad-setting | `set_settings` with bandwidth -1 | `SettingInvalid{key}`; settings unchanged | IT-14-15 |
| echo | `set_settings` with a bandwidth cap during a transfer | applied value returned; cap visible on the transfer within 1 s | IT-14-15, PT-14-4 |
| no-hold | `resolve_hold` on an active pair | `NoSuchHold` | IT-14-15 |
| logout-offline | `logout` with the server down | local token gone; `server_revoked = false` | IT-14-15 |
| events | `subscribe`, then `pause` and `resume` | two `StatusEvent`s, pair `paused` then `active` | IT-14-15 |

## Open points (flagged in the producer task)

1. `remove_pair` keeping local and server files is derived from "nothing is deleted except via the guard"; the specs do not state it for removal. Confirm.
2. `RestoreFromServer` for a hold caused by 200 or more remote deletes is not defined (14 §5 step 6 describes it as a re-download, which fits a local mass delete).
3. A pair in `root_missing` because its root is empty while the index is not: the mockup offers no button and says sync continues when the folder is back; whether `resolve_hold` can confirm that the emptying was intended is not stated.
4. The order of checks in `add_pair` is not specified; tests assert one error per single fault.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B10) |
