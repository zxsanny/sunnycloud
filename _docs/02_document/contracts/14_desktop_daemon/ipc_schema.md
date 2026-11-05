# Contract: ipc_schema — DaemonControl types, JSON-RPC wire and local endpoint

**Component**: 14 desktop_daemon (crate `ipc_schema`)
**Producer task**: E-24/c05 — `_docs/02_tasks/todo/B10-05_ipc_schema_crate.md` (types, method names, errors, wire mapping). The Transport section is implemented on the daemon side by E-24/c16 — `B10-18_ipc_server_endpoint.md`.
**Consumer tasks**: this batch: E-24/c11, E-24/c14, E-24/c15, E-24/c16, E-24/c17, E-24/c19, E-24/c20, E-24/c21 [v1.1], E-24/c22 [v1.1]; other batches: E-25 (stub daemon, `DaemonClient`, the six screens, UI suite), E-31 (desktop and security scenarios), E-24/p01..p03
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

The one schema between the daemon (14) and every local client of it: the settings window (15) over JSON-RPC and the tray in-process. It names the methods of `DaemonControl`, their parameter, result and error types, the JSON-RPC 2.0 mapping and the local endpoint rules. 15 depends on this crate and on nothing else of 14. No method returns a token, secret or credential (AC-48).

Sources: `components/14_desktop_daemon/description.md` §2, §5 (Local IPC), §15 risk 0; `components/15_desktop_settings/description.md` §2; `epics/E-24_desktop_daemon.md` Interface Specification; `module-layout.md` (crate `ipc_schema`); `ui_mockups/desktop-settings.html` (Status and Attention screens); `data_model.md` §11.

## Shape

### Transport (implemented by E-24/c16; checked by the 15 client)

| Item | Rule |
|---|---|
| Protocol | JSON-RPC 2.0. One UTF-8 JSON object per line, ended by `\n` (newline-delimited, as 15 §2 states). |
| Linux endpoint | Unix socket, mode 0600, inside a directory of mode 0700 under `$XDG_RUNTIME_DIR`, owned by the user running the daemon. |
| Windows endpoint | Named pipe whose security descriptor grants access only to the current user SID; remote clients are rejected; created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a pre-created pipe makes the daemon refuse the endpoint and report it. |
| macOS endpoint [v1.1] | Unix socket in the daemon's own 0700 directory. |
| Client trust check | Before sending anything the client checks that the endpoint belongs to the current user (Unix: socket owner and mode; Windows: pipe server SID) and refuses otherwise (15 §5, NFT-SEC-32). |
| Requests | Every method is a JSON-RPC request with an `id`; the method name is the `DaemonControl` method name below. |
| Events | After `subscribe`, the daemon sends JSON-RPC notifications (no `id`) on that connection; each carries one `StatusEvent`. |

### Methods (`DaemonControl`)

| Method | Params | Result | Errors |
|---|---|---|---|
| `status` | none | `DaemonStatus` | none |
| `set_server` | `{url: String}` | `null` | `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired{message}`, `SignedIn` |
| `list_server_folders` | `{parent: NodeId?, page: PageToken?}` (no parent = account root) | `FolderPage{items: [FolderEntry], next: PageToken?}` | `NotSignedIn`, `ServerUnreachable`, `UnknownFolder` |
| `list_pairs` | none | `[PairView]` | `NotSignedIn` |
| `add_pair` | `PairSpec` | `PairId` | `NotSignedIn`, `RootInvalid`, `RootNested`, `RulesInvalid{line}`, `CloudManagedRoot`, `AlreadyPaired`, `UnknownFolder` |
| `update_pair` | `{pair_id: PairId, patch: PairPatch}` | `null` | `UnknownPair`, `RulesInvalid{line}` |
| `remove_pair` / `pause` / `resume` | `{pair_id: PairId}` | `null` | `UnknownPair` |
| `list_skipped` | `{pair_id: PairId?, page: PageToken?}` | `SkippedPage{items: [SkippedEntry], next: PageToken?}` | `UnknownPair` |
| `list_escaped` | `{pair_id: PairId?, page: PageToken?}` | `EscapedPage{items: [EscapedEntry], next: PageToken?}` | `UnknownPair` |
| `get_settings` | none | `Settings` | none |
| `set_settings` | `SettingsPatch` | `Settings` (the applied values) | `SettingInvalid{key}` |
| `resolve_hold` | `{pair_id: PairId, decision: HoldDecision}` | `null` | `NoSuchHold` |
| `login_start` | `{device_name: String}` | `LoginPrompt` | `ServerNotSet`, `ServerUnreachable`, `HttpsRequired` |
| `login_status` | none | `LoginState` | none |
| `logout` | none | `LogoutOutcome` | none |
| `subscribe` | none | `null`, then `StatusEvent` notifications | none |

Every method may also answer `Internal{correlation_id}` for an unexpected failure; 15 shows a generic message plus that id (15 §2).

### Types

| Type | Shape | Source / note |
|---|---|---|
| `NodeId` | server node id, canonical UUID string | server ids are UUIDs |
| `PairId` | integer | `pair.id INTEGER` (data model §11) |
| `PageToken` | opaque string | paged listings |
| `FolderEntry` | `{node_id: NodeId, name: String}` | folders only |
| `PairSpec` | `{target: PairTarget, local_root: String, rules_text: String}` | exactly one of node id and path |
| `PairTarget` | `NodeId{node_id}` or `Path{server_path: String, create_if_missing: bool}` | a request carrying both is not decodable (see Errors) |
| `PairPatch` | `{rules_text: String?}` | proposed, open point 7 |
| `PairView` | `{pair_id, local_root, server_node_id: NodeId, state: PairState, rules_text}` | data model `pair` |
| `PairState` | `Active`, `Paused`, `HoldMassDelete`, `RootMissing` | 14 §5 state machine |
| `HoldView` | `{pair_id, kind: HoldKind, local_deletes: u32, remote_deletes: u32}` | mockup Attention screen shows both counts |
| `HoldKind` | `MassDelete`, `RootMissing` | 14 §5 step 6 |
| `HoldDecision` | `ConfirmDeletes`, `RestoreFromServer` | |
| `SkippedEntry` | `{pair_id, rel_path, reason: SkipReason}` | |
| `SkipReason` | `Placeholder`, `Symlink`, `Junction`, `SpecialFile` (socket, FIFO, device) | 14 §5 steps 7 and 9; mockup |
| `EscapedEntry` | `{pair_id, original_name, saved_name, rel_path}` | |
| `Settings` | `{bandwidth_limit: u64, hash_throttle: f64, autostart: bool, tray: bool}` | `bandwidth_limit` 0 = unlimited (mockup); unit open (G-AS-2) |
| `SettingsPatch` | every `Settings` field optional | |
| `DaemonStatus` | `{server_url: String?, account: AccountView?, pairs: [PairView], skipped_count: u64, escaped_count: u64, holds: [HoldView], update_available: UpdateView?, access_denied: [PairId], watcher_degraded: [PairId]}` | field set fixed by 14 §2; field types proposed (open point 3) |
| `AccountView` | `{account_id: String}` | proposed |
| `UpdateView` | `{version: String}` (present = an update is staged) | mockup shows the version |
| `StatusEvent` | `{status: DaemonStatus}` (the full new status) | 15 §2 |
| `LoginPrompt` | `{verification_url: String, user_code: String, expires_at: String (RFC 3339 UTC)}` | never a token |
| `LoginState` | `Pending`, `Approved`, `Denied`, `Expired`, `SignedOut` | |
| `LogoutOutcome` | `{server_revoked: bool}` (`false` = the server copy still needs revoking) | 14 §2 `logout` |

`access_denied` is filled only by the macOS daemon [v1.1]; it is always empty on Linux and Windows.

### Errors (`ControlError`)

Variants: `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired{message}`, `SignedIn`, `NotSignedIn`, `ServerNotSet`, `UnknownFolder`, `RootInvalid`, `RootNested`, `RulesInvalid{line: u32}` (1-based line of the rules text), `CloudManagedRoot`, `AlreadyPaired`, `UnknownPair`, `SettingInvalid{key}`, `NoSuchHold`, `Internal{correlation_id}`.

JSON-RPC mapping (proposed, open point 4):

| Case | JSON-RPC error |
|---|---|
| Any `ControlError` | `code` -32000, `message` = variant name, `data` = `{"kind": "<Variant>", ...variant fields}` |
| Line is not valid JSON | -32700 Parse error |
| Unknown method | -32601 Method not found |
| Params that do not decode to the method's type (including a `PairSpec` with both node id and path) | -32602 Invalid params |

## Invariants

- No type reachable from a method result or a `StatusEvent` is, contains or is named `Token`, `Secret`, `Credential`, `token`, `secret` or `password` (ST-14-2 schema walk).
- The crate depends on serde glue only (and helper error if wire codes are needed); never on `sync-core` or daemon internals (module-layout).
- Every `StatusEvent` carries a complete `DaemonStatus`; a client never merges partial updates.
- Readers ignore unknown object fields, so adding an optional field is a minor change.
- Method names, variant names and field names are the spellings above; both sides use this crate, never a copy.

## Non-Goals

- Not covered: what each method does inside the daemon, its timing and its state effects (`daemon_control.md`); 15's client-side errors `DaemonNotRunning` and `EndpointUntrusted`; the HTTP API of the server.

## Versioning Rules

- **Breaking**: a renamed or removed method, variant or field; a changed type; a new required field. Needs a new major version, and 14 and 15 change in the same release.
- **Non-breaking**: a new optional field, a new method, a new error variant that 15 shows as the generic message. Minor version bump.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| schema-walk | every type reachable from the method table and `StatusEvent` | 0 fields typed or named token, secret, credential, password | ST-14-2 |
| round-trip | one instance of each type | JSON encode then decode gives the same value | |
| both-targets | `add_pair` with a node id and a server path | -32602 Invalid params | IT-14-15 |
| rules-line | `RulesInvalid{line: 3}` | `data` = `{"kind":"RulesInvalid","line":3}` | IT-14-4, IT-15-4 |
| unknown-method | method `get_token` | -32601 | |
| event-shape | `StatusEvent` after `pause` | carries the full `DaemonStatus` with the pair in `Paused` | IT-14-15 |
| rules-text-verbatim | rules text with a trailing newline and non-ASCII | decoded string equal byte for byte | IT-15-4 |

## Open points (flagged in the producer task)

1. Endpoint names: the socket file and directory names and the pipe name are not fixed. 15 §1 says the path rule comes from helper 01, but helper 01's table and `sc_config` contract have none. Proposed: the rule lives in this crate as a pure function of the OS and the user's directories, and the names are written here before the contract is frozen.
2. Per-file errors have no carrier: FT-N-51 (a name the server rejects is "listed with the server's reason"), the mockup's Status "Errors" list and 15 §5 "errors" need a field or a list method. Needs a decision before freeze (E-24 with the owner).
3. Field types the specs leave open, proposed above: `account`, `update_available` (IT-14-17 says "true", the mockup shows the version), `access_denied` and `watcher_degraded` as lists of pairs (NFT-RES-40 and the mockup report per pair), `HoldView` counts per direction, `LogoutOutcome`.
4. JSON-RPC error codes and the notification method name for `StatusEvent` are proposed here, not taken from a spec.
5. `LoginPrompt.expires_at` needs the grant's lifetime, but 13's `DeviceAuthorization{user_code, verification_uri, interval}` has no expiry field (E-23/c13).
6. `bandwidth_limit` has no unit (G-AS-2, owner; the mockup shows KB/s).
7. `update_pair` takes a "partial spec"; which fields may change is not stated. Proposed: the rules text only.
8. No maximum line length is stated for a frame; 15 rejects malformed frames (ST-15-1) but neither side names a size cap.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B10) |
