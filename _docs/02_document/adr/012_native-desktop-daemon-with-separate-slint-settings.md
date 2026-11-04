# ADR-012: Native desktop daemon with a separate Slint settings process

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

The desktop client must sync 100,000 files on Linux and Windows (macOS in v1.1) with little memory and CPU, keep working when no tray exists (GNOME), and still give the user a settings window that opens fast and uses the operating system's own file dialogs. Two budgets pull in opposite directions: the daemon is capped at 100 MB and 1% idle CPU, while a UI toolkit with a GPU or web stack costs 120-400 MB. Draft 01 picked Tauri 2 + Svelte for the settings window; the second research round measured Slint's software renderer and the owner made Slint primary with Tauri as fallback (`06_component_fit_matrix.md` § Round 2 changes).

- Acceptance criteria addressed: AC-15 (100,000 files, at most 1% idle CPU and 100 MB; settings window not counted), AC-16 (settings window opens within 1 s cold at p95 on a 2019 Windows HDD laptop and Ubuntu LTS Wayland with NVIDIA, no environment-variable workarounds; a setting reaches the daemon within 5 s), AC-17 (native dialogs, no custom file browser), AC-26 (syncs fully with no tray; Windows logon task), AC-48 (IPC closed to other users and remote clients, no token-returning method)
- Restrictions addressed: RESTRICT-06 (native Rust daemon with tray, Slint settings window, Tauri 2 fallback, native dialogs, typed patterns), RESTRICT-25 (daemon runs at login without UI), RESTRICT-29 (Slint GPL-3.0 compatible with AGPL), RESTRICT-05 (Rust for daemons)
- Risks addressed: R25 (Linux token fallback is a 0600 file), R26 (rename or lost index causes mass-delete hold), R27 (100 MB budget versus buffers, hashing, SQLite), R28 (inotify watch limit forces rescan-only pairs)
- Research source: `_docs/00_research/02_fact_cards/mve_round2.md` § MVE — Slint 1.18.1; `_docs/00_research/raw/security_clients.md` § Options comparison, 5. Desktop UI toolkit

## Decision

We will build the desktop client as a native Rust daemon with a tray, plus a separate, on-demand Slint settings process that talks to the daemon over a local JSON-RPC socket and uses native dialogs through `rfd`.

The daemon (component 14) is one process on one tokio runtime: `notify` 8.2.1 as a hint source, `rusqlite` 0.41.1 (bundled, WAL) as the `LocalIndex`, `reqwest` 0.13.5 through `sync-core`, `tray-icon` 0.26.1 with the ksni backend on Linux, with a missing tray host logged and non-fatal. It owns all state and settings. The settings window (component 15) is a stateless view: Slint 1.18.1 with the software renderer only (no femtovg, no skia), `rfd` 0.17.2 for folder and file pickers, and no custom folder browser. Both depend on the `ipc_schema` crate, which the daemon owns; control goes through the `DaemonControl` trait (`status`, `add_pair`, `list_server_folders`, `login_start`, `resolve_hold`, `subscribe`, and others) and no method returns a token. The socket is JSON-RPC 2.0 via `interprocess`: mode 0600 in a 0700 directory on Linux and macOS, a named pipe with a current-user security descriptor on Windows.

Slint is used under its GPL-3.0-only option. Tauri 2 + Svelte becomes the settings window if any fallback trigger fires: cold start over 1 s at p95 on the weakest Windows laptop or Ubuntu GNOME Wayland; an unfixed Windows or macOS blocker; a screen-reader failure; an `rfd` focus or deadlock problem; a licence change.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| Electron for the settings window | 150-400 MB idle (`raw/b_clients_media.md` notes) and contradicts "native Rust"; the daemon budget is unaffected but the settings process would be 9-24 times the Slint figure |
| Tauri 2 only, daemon and window in one process | Webview lifecycle may inflate daemon RSS (unmeasured) and Linux tray events are missing in that shape (`security_clients.md` option 3); no official cold-start figure for WebKitGTK/WebView2 against AC-16 |
| One process: daemon with an in-process Slint window | The 100 MB budget counts only a separate window as excluded (AC-15); the GL path measured 121-136 MB RSS, and a daemon that must run with no tray or display (AC-26) should not link a UI toolkit |
| Settings window in egui or iced | egui with GPU measured about 135 MB RSS against about 30 MB for Slint software (one uncontrolled case, `security_clients.md` fact 51); iced unmeasured |
| No UI: headless watcher with a config file or CLI | Fails AC-16 and AC-17: no settings window and no native folder dialog; a hand-edited file also bypasses typed pattern validation (AC-66) |

## Consequences

### Positive

- Measured on Linux Wayland with NVIDIA and no environment variables: software-only Slint opens in 47-61 ms with about 17 MB RSS and a 12.3 MB binary, against 157-424 ms and 121-136 MB with OpenGL (AC-16 headroom).
- The 100 MB budget (AC-15, R27) applies to the daemon alone; the window exists only while the user needs it.
- The daemon runs headless and without a tray (AC-26); nothing in it depends on a display toolkit.
- One IPC schema crate keeps the two binaries in step and gives one place to enforce AC-48.

### Negative

- Two binaries and two toolchains (Rust and `.slint`) to build, sign, package and update.
- Windows and macOS cold start, fractional DPI, the rfd dialog on those OSes and screen-reader behaviour are unmeasured; gate S3 must run before release.
- Excluding the GL renderer is mandatory: the documented software fallback did not rescue a simulated EGL failure (`mve_round2.md` Slint renderer observation).
- On Linux `rfd` needs an XDG portal backend or `zenity`; wlroots portals are unsupported.
- Slint's GPL-3.0-only route blocks a future proprietary build unless its royalty-free or commercial terms are taken.

### Neutral / Open

- The daemon's 100 MB, inotify limits (R28) and per-OS watcher behaviour are gated by S2, not by this decision.
- The updater (AC-49), its manifest layout and the two pinned keys live in the daemon, not in the settings window; the manifest layout is a separate ADR item.
- A Slint-to-Tauri switch changes component 15 only; the `DaemonControl` contract stays.

## Evidence

- `_docs/02_document/architecture.md` § Component list (14, 15); § 2 Technology Stack (Desktop); § 5 Integration Points (Desktop daemon to Settings window)
- `_docs/02_document/components/14_desktop_daemon/description.md` § 1 High-Level Overview; § 2 Internal Interfaces (DaemonControl); § 5 Implementation Details (Local IPC, Tray)
- `_docs/02_document/components/15_desktop_settings/description.md` § 1 High-Level Overview; § 11 Security notes; § 15 Risks
- `_docs/02_document/component-decisions.md` § Final component list
- `_docs/00_research/06_component_fit_matrix.md` § Round 2 changes (Desktop settings UI)
- `_docs/00_research/02_fact_cards/mve_round2.md` § MVE — Slint 1.18.1 (Measurements; Fallback triggers)
- `_docs/00_research/raw/security_clients.md` § Options comparison, 5. Desktop UI toolkit; `_docs/00_research/raw/b_clients_media.md` (Electron note)
- `_docs/00_research/03_comparison_framework.md` § Initial Population (Desktop tray, Settings UI)
- `_docs/02_document/risk_mitigations.md` § Risk Register (R25 to R28)

## Notes

Re-evaluate the toolkit after gate S3 on the weakest Windows laptop and Ubuntu GNOME Wayland. The Linux token fallback (R25) and the inotify guide (R28) are accepted and documented, not solved here.
