# MVE fact cards: desktop client candidates

Accessed 2026-11-02/03. Versions are from crates.io API (max stable) and docs.rs/GitHub source read on those dates. "Verify" = not established from official docs; needs a spike on the named OS. Local source clones were read for items marked (src).

Binding-table legend: Pass = official doc/source supports it; Verify = plausible but unmeasured or undocumented; Fail = contradicted.

---

## MVE — notify + notify-debouncer-full in recursive-watch mode with file-id cache
- **Source**: https://docs.rs/notify/latest/notify/ (8.2.1 = crates.io max stable); https://docs.rs/notify-debouncer-full/latest/notify_debouncer_full/ (0.7.0 stable, depends `notify ^8.2.0`); GitHub main (src) https://github.com/notify-rs/notify: notify 9.0.0-rc.7 (2026-10-28), debouncer-full 0.8.0-rc.2; MSRV 1.88; licences: notify CC0-1.0, debouncer-full/file-id/notify-types MIT or Apache-2.0. Accessed 2026-11-03.
- **Pinned mode**: We will use `notify-debouncer-full` 0.7.0 on `notify` 8.2.1 `RecommendedWatcher` (inotify / FSEvents / ReadDirectoryChangesW), `RecursiveMode::Recursive` on each sync root, `RecommendedCache` (file-id cache), debounce timeout 2 s, with inputs = one watched root per sync folder and outputs = debounced create/modify/remove/rename events plus a rescan signal, on Linux, macOS and Windows. The 9.0.0-rc line is not pinned (pre-release).
- **Inputs in the example**: `new_debouncer(Duration::from_secs(2), None, handler)` then `debouncer.watch(".", RecursiveMode::Recursive)`; handler receives `DebounceEventResult` (Ok(events) / Err(errors)) (debouncer-full crate docs example).
- **Outputs in the example**: a batch `Vec<DebouncedEvent>`; "A single `Rename` event is emitted only when the From and To events can be matched"; duplicate creates dropped; no `Modify` after `Create`; one `Remove` per deleted directory on inotify.
- **Project inputs**: one or more roots holding up to 100,000 files; files written by the daemon itself (downloads); network or removable volumes possible.
- **Project outputs required**: (a) change events after a 2 s quiet period (AC-18); (b) rename detection; (c) a reliable "events were lost, rescan" signal; (d) no echo loop for our own writes.
- **Match assessment**: partial. Recursive watching, rename pairing and debouncing match. (1) Self-write filtering: no per-event "own write" flag on Linux/Windows; FSEvents own-process marking exists only in 9.0.0-rc source (`events marked as originating in this process carry its process ID`, src fsevent.rs). Echo suppression must be done by us (file id + size + mtime + hash). (2) Overflow signal: in the pinned stable 8.2.1 source, inotify emits `Flag::Rescan` on `Q_OVERFLOW` (docs.rs source inotify.rs), but the Windows backend (windows.rs 8.2.1) contains no `Rescan`/overflow handling at all (an unrecognised error code leads to "log and unwatch the directory"), so a Windows buffer overflow is silent in 8.2.1; Windows rescan signalling (`completion_rescan_event`) exists only on main / 9.0.0-rc (src). FSEvents `MustScanSubDirs` -> `Flag::Rescan` is in main (src fsevent.rs); not checked for 8.2.1. debouncer-full 0.7.0 does handle `need_rescan()` (stores one `rescan_event`, calls `cache.rescan(&roots)`, emits it after the timeout; docs.rs source lib.rs). (3) Network/removable volumes: not watchable reliably. (4) Memory of the file-id cache at 100k files is not documented.
- **If partial or mismatch**: Network: "Network mounted filesystems like NFS may not emit any events for notify to listen to." (notify docs, Known Problems; workaround "the PollWatcher backend"). Large trees: "When watching a very large amount of files, notify may fail to receive all events." and "the linux backend is documented to not be a 100% reliable source." Linux limit: "Note that for recursive watched folders each file and folder inside counts towards the limit" and `max_user_watches` can be raised via sysctl; ENOSPC is mapped to `ErrorKind::MaxFilesWatch` and arrives as an `Err` on the event channel (src inotify.rs). Windows: Win32 `ReadDirectoryChangesW` buffer overflow discards the buffer and returns 0 bytes; notify maps `ERROR_NOTIFY_ENUM_DIR` / zero-byte completion to a path-less `Flag::Rescan` event (src windows.rs `completion_rescan_event`). FSEvents renames use `RenameMode::Any` ("FSEvents does not pair the old and new paths", src), which is why the debouncer's file-id cache is needed on macOS and Windows ("Optionally keeps track of the file system IDs all files and stitches rename events together (macOS FS Events, Windows)", debouncer docs). Rename modes: `From`, `To`, `Both` ("The paths should be provided in this exact order (from, to)", notify-types 2.2.0). Removable volumes: no statement found in official docs; treat as Verify.
- **Restriction/AC binding**:

| Restriction or AC id | Status | Evidence |
|---|---|---|
| AC-15 CPU idle <=1%, <=100 MB at 100k files | Verify | Memory/CPU of the watcher plus `FileIdMap` at 100k files is not documented; kernel inotify watch count is per directory/file per the doc sentence above. Needs a per-OS spike |
| AC-15 first scan <=60 s | Verify | The scan is our own walk+stat (0.3 s warm measured on tmpfs in 00_ac_assessment); notify does not provide the initial scan |
| AC-18 settle 2 s (file unchanged for 2 s) | Fail as sole mechanism | debouncer-full 0.7.0 source: each event is stamped on arrival and emitted once its own age >= timeout; "A later event on the same path is appended ... and does not extend the age of earlier events" (read from lib.rs). A file still being written can therefore be reported mid-write. We must add our own stat-stable check (size+mtime unchanged for 2 s) before upload |
| AC-18 no echo loop | Verify | No own-write flag on Linux/Windows in the pinned stable; our own suppression logic is required |
| Rescan signal for lost events | Linux Pass; Windows Fail on 8.2.1 (Pass on 9.0.0-rc.7); macOS Verify | 8.2.1 inotify.rs emits `Flag::Rescan` on `Q_OVERFLOW`; 8.2.1 windows.rs has no Rescan/overflow code; main has `completion_rescan_event`. Mitigation: periodic full rescan (cheap walk) on every OS plus rescan on any `Err`/`Rescan` |
| Platform (daemon must work on all three OSes) | Pass | notify README: Linux inotify, macOS FSEvents, Windows ReadDirectoryChangesW |

  Other restrictions and ACs: N/A.
- **Proposed status**: Selected (conditional): use it as a hint source only. Settle check, echo suppression and the full rescan (startup, any `Rescan`/`MaxFilesWatch` error, and a periodic safety rescan, mandatory on Windows while on 8.2.1) are ours; network/removable roots use periodic scan or `PollWatcher`.

---

## MVE — tray-icon (+ muda menus) with the `ksni` backend on Linux, tao event loop on macOS/Windows
- **Source**: https://docs.rs/tray-icon/latest/tray_icon/ (0.26.1, crates.io max stable, updated 2026-10-07); https://github.com/tauri-apps/tray-icon README + CHANGELOG + src (src); https://docs.rs/muda/latest/muda/ (0.21.2); https://docs.rs/ksni/latest/ksni/ (0.3.7, Unlicense; zbus ^5; tokio default or async-io; `blocking` feature) + https://github.com/iovxw/ksni (src); tao 0.37.2 `ActivationPolicy` (src). Licences: tray-icon and muda MIT or Apache-2.0 (repo LICENSE files). Accessed 2026-11-03.
- **Pinned mode**: We will use `tray-icon` 0.26.1 with the `ksni` cargo feature on Linux (no GTK, no libappindicator), the native backends on macOS (NSStatusItem) and Windows (Shell_NotifyIcon), `muda` 0.21.2 menus (via tray-icon), and a `tao` 0.37.2 event loop on the main thread on macOS/Windows, with inputs = icon images per state, tooltip, menu items (Pause/Resume, Open folder, Open settings, Quit) and outputs = `MenuEvent`s forwarded to the daemon core, on macOS, Windows and Linux. The daemon core runs on a tokio runtime on other threads. If tray creation fails or no tray host exists the daemon continues without it.
- **Inputs in the example**: `TrayIconBuilder::new().with_menu(Box::new(tray_menu)).with_tooltip(..).with_icon(icon).build()`; events via `MenuEvent::receiver()` / `TrayIconEvent::receiver()` or `set_event_handler` + event-loop proxy (README, docs).
- **Outputs in the example**: a tray icon with a context menu; click and menu events delivered to the receiver; `set_icon`, `set_tooltip`, `set_menu`, `set_visible` exist on `TrayIcon` (src lib.rs).
- **Project inputs**: headless Rust daemon; states Idle/Syncing/Paused/Error; menu actions pause/resume, open folder, open settings, quit.
- **Project outputs required**: a visible tray icon with menu on macOS, Windows and (where a host exists) Linux; full sync with no tray (AC-26).
- **Match assessment**: partial. Matches on all three OSes with a hard event-loop constraint per OS and no guarantee of visibility on stock GNOME.
- **If partial or mismatch**: README: "On Windows and the Linux/FreeBSD AppIndicator backend, an event loop must be running on the thread. The KSNI backend manages its own worker thread." and "On macOS, an event loop must be running on the main thread so you also need to create the tray icon on the main thread." muda: "menus can only be used from the main thread" on macOS. Linux: ksni returns `Error::WontShow` ("no StatusNotifierHost exists") unless `assume_sni_available(true)`; tray-icon's ksni backend sets `assume_sni_available(true)` (src platform_impl/ksni/mod.rs), so on GNOME without the AppIndicator extension the build succeeds and the icon simply never appears; tray-icon 0.26.0 changelog: the KSNI backend "no longer fails to build a tray icon when no `org.kde.StatusNotifierWatcher` is on the session bus yet. The icon registers when the watcher appears". Failure to reach the D-Bus session bus is a returned error (`spawn()?`), which the daemon must treat as non-fatal. ksni docs on watcher loss: user restarted GNOME shell or disabled the tray plugin; "Consider setting a fallback tray" (OfflineReason::No). macOS Dock icon hiding is done through tao `ActivationPolicy::Accessory` (enum exists in tao src; behaviour on a LaunchAgent Verify).
- **Restriction/AC binding**:

| Restriction or AC id | Status | Evidence |
|---|---|---|
| Restrictions "Desktop UI: headless daemon with tray (`tray-icon`, `ksni`)" | Pass | tray-icon 0.26.1 offers both: `ksni` feature is the GTK-free StatusNotifierItem backend (0.25.0 changelog) |
| AC-26 sync with no tray | Pass if tray init is non-fatal | ksni backend with `assume_sni_available(true)`: no error without a host; `spawn()` D-Bus error must be caught by us (design condition) |
| Platform: GNOME shows no tray | Pass | Same; verified only by source reading, Verify on a stock GNOME VM |
| AC-16 settings window open <=1 s | N/A here (see Tauri block) | menu "Open settings" only spawns the process |
| AC-15 CPU/RAM | Verify | tao event loop plus tray adds an idle loop on macOS/Windows; not documented, needs a spike |

  Other restrictions and ACs: N/A.
- **Proposed status**: Selected: tray-icon (ksni feature on Linux) is the single tray crate; a separate direct `ksni` dependency is not needed. Keep `ksni` direct as the documented fallback if the tray-icon ksni backend misbehaves.

---

## MVE — Tauri 2 as an on-demand settings process (separate binary, Svelte SPA, local-socket client)
- **Source**: https://v2.tauri.app/start/prerequisites/ ; https://v2.tauri.app/reference/webview-versions/ ; https://v2.tauri.app/plugin/single-instance/ ; https://v2.tauri.app/plugin/dialog/ ; https://v2.tauri.app/start/frontend/sveltekit/ ; https://v2.tauri.app/start/create-project/ ; crates.io: tauri 2.12.5 (2026-10-21), tauri-plugin-single-instance 2.5.5, tauri-plugin-dialog 2.8.4 (Cargo.toml in plugins-workspace v2: `rfd 0.17`, features `gtk3` default / `xdg-portal`), interprocess 2.4.7 (Cargo.toml src: licence 0BSD OR Apache-2.0, feature `tokio`), tokio named-pipe docs (tokio 1.54.1). Issue evidence: https://github.com/tauri-apps/tauri/issues/5889. Accessed 2026-11-03.
- **Pinned mode**: We will use Tauri 2.12.5 as a separate settings executable that the daemon spawns with `std::process::Command` on "Open settings", with `tauri-plugin-single-instance` 2.5.5 registered first, `tauri-plugin-dialog` 2.8.4 for folder/file pickers, a Svelte 5 + Vite SPA (`frontendDist`), and Rust-side Tauri commands that talk to the daemon through `interprocess` 2.4.7 `local_socket` (tokio feature; Unix domain socket on Linux/macOS, named pipe on Windows) using newline-delimited JSON-RPC 2.0 written with serde_json (no JSON-RPC framework), with inputs = user settings edits and outputs = settings applied in the daemon, on macOS, Windows and Linux.
- **Inputs in the example**: single-instance `init(|app, args, cwd| ..)`; dialog `blocking_pick_file()` / non-blocking `pick_file()`; SvelteKit/Vite static build served from `frontendDist`; interprocess `ListenerOptions` / `ConnectOptions`.
- **Outputs in the example**: second launch notifies the first instance and exits (Linux via D-Bus); dialog returns file system paths on Linux/Windows/macOS; local socket "Unix uses Unix-domain sockets, and Windows uses named pipes".
- **Project inputs**: tray menu click spawns the settings process; settings edits (folders, include/exclude patterns, pause); status read-back.
- **Project outputs required**: window visible <=1 s (AC-16); change applied in daemon <=5 s without restart (AC-16); native dialogs only (AC-17).
- **Match assessment**: partial. Functionality matches on all three OSes; AC-16 cold-start time is not established by any official source; Linux needs system WebKitGTK.
- **If partial or mismatch**: No official Tauri document gives start-up time or memory figures ("Concept: App size" page covers only binary size). Issue-tracker evidence (Tauri 1 era, Dec 2022, issue #5889): default app Ubuntu 22.04 USS 125 MB / PSS 185 MB; Postman-scale test Tauri 421 MB macOS, 581 MB Ubuntu, 399 MB Windows (page does not state whether RSS). Microsoft Learn (WebView2 performance concepts) says a cold start must start WebView2 processes and caches, so a delay is expected; no millisecond figure found. Webview: "Tauri uses WebView2 ... Chromium" on Windows, "WebKit on macOS (through WKWebView)", "`webkit2gtk`" on Linux; Debian prerequisites list `libwebkit2gtk-4.1-dev`; Windows 10 1803+ already includes WebView2, otherwise the Evergreen Bootstrapper. Single-instance: "On Linux the Single Instance plugin uses DBus"; "it won't work as intended for snap or flatpak packages by default"; mechanism on Windows/macOS not documented (Verify). Frontend: Svelte is a create-tauri-app template (TypeScript or JavaScript); SvelteKit needs `@sveltejs/adapter-static` with `fallback: 'index.html'` and `ssr = false` ("Tauri doesn't support server-based solutions"); a plain Svelte+Vite page is not described separately in the fetched docs (Vite page documents `frontendDist: ../dist`). Windows named pipes: interprocess has a Windows `SecurityDescriptor` option on the listener and a `PIPE_REJECT_REMOTE_CLIENTS` default (src create_instance.rs); Unix socket file mode is settable (src options.rs). interprocess "adds no framing", so framing (newline JSON) is ours. jsonrpsee 0.26.1 documents only HTTP/WebSocket transports, so a hand-rolled JSON-RPC over the socket is simpler than a custom transport.
- **Restriction/AC binding**:

| Restriction or AC id | Status | Evidence |
|---|---|---|
| AC-16 window opens <=1 s | Verify (likely at risk on Windows cold start and WebKitGTK) | No official figure; WebView2 cold-start caveat above. The 00_ac_assessment fallback (pre-spawned hidden window) is compatible with single-instance |
| AC-16 setting applied <=5 s | Pass by design | Local socket RPC, daemon applies on receipt; no library limit; Verify end to end |
| AC-17 native dialog for any file/folder | Pass | tauri-plugin-dialog uses `rfd` on desktop (plugin Cargo.toml); returns "file system paths" on Linux, Windows, macOS; no custom browser needed |
| AC-15 daemon footprint excludes settings process | Pass | Separate process by design (AC-15 text) |
| Restrictions "settings process (Tauri 2 with Svelte, rfd dialogs) talking to the daemon over a local socket" | Pass | Components exist as stated; Slint fallback remains if AC-16 fails |
| Platform packaging | Verify | Linux build needs libwebkit2gtk-4.1 and GTK3 (dialog plugin default `gtk3`); snap/flatpak single-instance needs D-Bus permissions |

  Other restrictions and ACs: N/A.
- **Proposed status**: Experimental only until a cold-start spike on the weakest Windows machine and a WebKitGTK Linux VM shows <=1 s (AC-16); the restrictions already name Slint as the fallback.

---

## MVE — rfd (Rust File Dialog) standalone, async folder picker (no Tauri)
- **Source**: https://docs.rs/rfd/latest/rfd/ (0.17.2 = crates.io max stable, updated 2026-10-22; MIT); https://github.com/PolyMeilex/rfd README. Accessed 2026-11-03.
- **Pinned mode**: We will use `rfd` 0.17.2 `AsyncFileDialog::pick_folder()` / `pick_folders` with the default `xdg-portal` feature on Linux (zenity fallback), from a process that owns a GUI event loop, with inputs = starting directory and title, and output = a chosen directory path, on macOS, Windows and Linux. In the planned architecture this is reached only indirectly through `tauri-plugin-dialog` (which pins rfd 0.17 with `gtk3`), not called from the daemon.
- **Inputs in the example**: builder `AsyncFileDialog::new()...` then `pick_file().await` (docs example).
- **Outputs in the example**: a selected file handle/path; feature table: SingleFile, MultipleFile, Filters, Async supported on Windows, macOS, Linux & BSDs, WASM32; "`pick_folder` is supported on Linux, Windows, and macOS, but not on WASM32".
- **Project inputs**: choose a sync folder, include/exclude file; native OS dialog only.
- **Project outputs required**: a native dialog on all three OSes (AC-17).
- **Match assessment**: partial for standalone use outside a windowed app; exact inside Tauri.
- **If partial or mismatch**: "macOS async dialogs require an `NSApplication` instance." Without one the dialog falls back to a sync dialog; "Non-windowed apps will never be able to spawn async dialogs or from threads other than the main thread." Linux: the XDG Desktop Portal backend is the default and "uses the GTK or KDE dialog, depending on the desktop environment"; it needs `libdbus`, else zenity is used; the `gtk3` feature switches to GTK3 and needs GTK3 headers. Docs do not describe use outside a GUI event loop on Linux/Windows (Verify). Version note: crates.io shows 0.17.2 (2026-01-12) as highest and a later 0.16.1 patch (2026-10-22) of the older line; neither is yanked, so pin `0.17` to match tauri-plugin-dialog.
- **Restriction/AC binding**:

| Restriction or AC id | Status | Evidence |
|---|---|---|
| AC-17 native dialog | Pass | rfd: "cross platform Rust library for using native file open/save dialogs"; folder pick on 3 OSes |
| Restrictions "rfd dialogs" in settings process | Pass via tauri-plugin-dialog (rfd 0.17) | plugin Cargo.toml |
| AC-26 / daemon without UI | N/A, daemon never opens dialogs | design |

  Other restrictions and ACs: N/A.
- **Proposed status**: Selected as a transitive component inside the settings process (via tauri-plugin-dialog); standalone use in the daemon is Rejected (macOS needs NSApplication, not needed).

---

## MVE — keyring 4.2.0 (`v1` mode) storing one device token per account
- **Source**: https://docs.rs/keyring/latest/keyring/ (4.2.0, 2026-08-29, MIT OR Apache-2.0, MSRV 1.88); GitHub open-source-cooperative/keyring-rs Cargo.toml + src/v1.rs (src); store crates: apple-native-keyring-store 1.0.2, windows-native-keyring-store 1.1.0, zbus-secret-service-keyring-store 1.0.1, linux-keyutils-keyring-store 1.0.0, db-keystore 0.6.0 (docs.rs). Accessed 2026-11-03.
- **Pinned mode**: We will use `keyring` 4.2.0 with the default `v1` feature (`Entry::new(service, user)`, `set_secret`/`get_secret`/`delete_credential`) with input = device token bytes (service `sunnycloud`, user = account id) and output = the same bytes read back by the daemon, on macOS (Keychain Services), Windows (Credential Manager) and Linux (Secret Service via zbus). Headless Linux without a Secret Service needs a separate fallback decision (below).
- **Inputs in the example**: `Entry::new("svc","usr")?`, `set_password`/`set_secret`, `Entry::store_status()` to learn whether the store initialised (src v1.rs).
- **Outputs in the example**: v1 docs: "On macOS, the secure credential store is Keychain Services. On Windows, ... the Windows Credential Manager. On *nix operating systems, the secure credential store is the Secret Service."
- **Project inputs**: a long-lived device token (RFC 8628 grant result), written once at login, read at every daemon start from a LaunchAgent / systemd user unit / logon task with no UI.
- **Project outputs required**: token survives reboot and is readable without user interaction (restrictions: "Device tokens are stored in the OS keychain").
- **Match assessment**: partial. exact on macOS and Windows; Linux depends on a running, unlocked Secret Service.
- **If partial or mismatch**: v1 mode on Linux hard-wires `zbus_secret_service_keyring_store::Store::new()` (src v1.rs); `Entry::new` returns `Error::NoDefaultStore` if initialisation failed. zbus store docs: "known issues with getting zbus and secret-service and the gnome keyring to work properly in headless environments"; items go to the default (login) collection; "There is no default collection on WSL"; KDE Wallet handles UTF-8 only so binary secrets must be encoded. keyring docs themselves say apps that need control over stores "should instead be linking to the keyring-core library and any specific credential stores". Fallback options found: `linux-keyutils-keyring-store` 1.0.0 "is completely in-memory and will not persist across reboots", credentials "expire after a few days" and must be re-obtained, so it cannot hold a durable device token; `db-keystore` 0.6.0 is an "Encrypted SQLite credential store ... backed by turso" (default `$XDG_STATE_HOME/keystore.db`), but "does not say how the key is generated, stored, or protected" and pins `turso =0.8.1`, so it moves the secret-at-rest problem to us. Windows store default persistence is "Enterprise" ("By default, created credentials have Enterprise persistence"), configurable only through the store's `persistence` modifier, which v1 mode does not expose, so selecting Local persistence needs keyring-core plus windows-native-keyring-store directly. apple-native-keyring-store docs say nothing about access prompts for changed or unsigned binaries or LaunchAgent use (Verify on macOS with the signed daemon).
- **Restriction/AC binding**:

| Restriction or AC id | Status | Evidence |
|---|---|---|
| Authentication: "Device tokens are stored in the OS keychain" | Pass on macOS, Windows; Verify on Linux | v1 docs above |
| AC-26 daemon syncs with no UI/tray (headless Linux login, no keyring) | Fail without a fallback | zbus store headless caveat; keyutils non-persistent |
| Platform: starts at login without UI | Verify | Keychain / Secret Service must be unlocked at login; no doc statement for LaunchAgent / systemd user context |

  Other restrictions and ACs: N/A.
- **Proposed status**: Needs user decision. Use the OS stores on macOS and Windows; for Linux with no Secret Service the user must choose between (a) device re-login at each reboot, (b) a 0600 token file under `$XDG_STATE_HOME`, (c) db-keystore. No fallback is verified as durable and secure from official docs.

---

## MVE — Per-user autostart: systemd user unit (Linux), LaunchAgent (macOS), Task Scheduler logon task (Windows)
- **Source**: systemd: https://www.freedesktop.org/software/systemd/man/latest/systemd.special.html , .../loginctl.html , .../systemd.unit.html , .../user@.service.html; Apple: https://developer.apple.com/documentation/servicemanagement/smappservice.md and .../updating-your-app-package-installer-to-use-the-new-service-management-api.md , launchd.plist(5) (https://keith.github.io/xcode-man-pages/launchd.plist.5.html); Microsoft: schtasks create (https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/schtasks-create), LogonType element, Logon trigger example, Run/RunOnce keys; crates: auto-launch 0.6.0 (docs.rs), objc2-service-management 0.3.2 (crates.io). Accessed 2026-11-03.
- **Pinned mode**: We will use (Linux) a unit file `~/.config/systemd/user/<name>.service` installed with `systemctl --user enable` and `WantedBy=default.target`; (macOS) a per-user launchd agent, registered with `SMAppService.agent(plistName:)` when the daemon ships inside a signed app bundle (plist in the bundle, called through `objc2-service-management`), with a plist in `~/Library/LaunchAgents` as the non-bundle fallback; (Windows) a Task Scheduler task with a Logon trigger for the current user and principal `LogonType = InteractiveToken`, created from an XML definition (`schtasks /create /xml`) or the COM API, with inputs = daemon executable path and outputs = daemon running in the user's session at each login with no UI, on Linux, macOS and Windows. We will not use the `auto-launch` crate or the Windows Run key.
- **Inputs in the example**: SMAppService sample: `SMAppService.agent(plistName: "com.xpc.example.agent.plist")` then `register()`; schtasks: `/sc onlogon /tn .. /tr .. [/ru user] [/it]`; Task Scheduler scripting example: logon trigger with `UserId`, `ExecAction`, `RegisterTaskDefinition`.
- **Outputs in the example**: agent registered "so it can begin launching subject to user approval"; task "runs whenever a user logs on"; systemd `default.target`: "the main target of the user service manager, started by default when the service manager is invoked".
- **Project inputs**: a per-user daemon that must start at login with no UI interaction and own a tray icon where available (Windows: a service cannot own a tray icon).
- **Project outputs required**: daemon running after each login on all three OSes (Platform restriction, AC-26 Windows logon task).
- **Match assessment**: partial. Each mode starts a per-user process at login; macOS requires user approval in System Settings for SMAppService agents; systemd user manager lifetime follows login unless lingering; auto-launch does not implement the Windows logon task.
- **If partial or mismatch**: auto-launch 0.6.0 docs: Windows "Adds a registry entry under the ...\CurrentVersion\Run key"; macOS "Launch Agent (default) ... or AppleScript"; Linux "XDG Autostart by default, or systemd via `LinuxLaunchMode::Systemd`" - so it cannot create the required Windows logon task. Run key docs: "The system does not provide guarantees about how promptly the programs in the `Run` key are run. ... the system may choose to delay the execution". Windows `LogonType`: "InteractiveToken: User must already be logged on. The task will be run only in an existing interactive session." and "S4U: ... no access to the network or encrypted files" (so S4U is unsuitable for a networked tray daemon). schtasks `/it`: run "only when the run as user ... is logged on"; without `/ru`, ONLOGON "runs whenever a user (any user) logs on", so the user must be named; whether a standard (non-admin) user may register a task for themselves is not stated in these docs (Verify); schtasks docs also say it "always prompts for a password unless you provide one" which is why the XML/COM path with InteractiveToken is pinned (Verify no prompt). macOS: SMAppService is "macOS: 13.0.0 -"; it is "a replacement for installing property lists in `~/Library/LaunchAgents` or `/Library/LaunchAgents`"; the sample places the plist in the helper's bundle in a "fully codesigned app bundle"; users manage it under System Settings > General > Login Items; third-party sources add `requiresApproval` status and Apple DTS statement that agent registration is per current user (forum, not official reference). launchd.plist(5): `RunAtLoad` default false and "should be avoided" as a speculative launch, `KeepAlive` implies RunAtLoad, `~/Library/LaunchAgents` = "Per-user agents provided by the user". Linux: `loginctl enable-linger`: "a user manager is spawned for the user at boot and kept around after logouts"; the fetched man pages do not state "started at first login" explicitly, so the default lifetime is Verify. A tray from a systemd user unit started before the panel is handled by ksni/tray-icon (watcher may appear later).
- **Restriction/AC binding**:

| Restriction or AC id | Status | Evidence |
|---|---|---|
| Platform: systemd user service (Linux) | Pass | user unit dir `$XDG_CONFIG_HOME/systemd/user` (systemd.unit); `default.target` is the user manager's main target |
| Platform: LaunchAgent (macOS) | Pass | SMAppService.agent docs; plist-in-LaunchAgents still the legacy equivalent per same page |
| Platform: Windows logon task, not a service | Pass | Logon trigger + InteractiveToken documented; "a Windows service cannot own a tray icon" is already a restriction |
| AC-26 "On Windows it starts through a logon task" | Pass (design), Verify (standard-user registration without prompt) | schtasks/Task Scheduler docs above |
| macOS first-run approval in Login Items | Verify | `register()` is "subject to user approval"; this is a UI step at install, not at each login |

  Other restrictions and ACs: N/A.
- **Proposed status**: Selected (three native mechanisms, ~tens of lines each); `auto-launch` Rejected because it lacks the Windows logon task.

---

## MVE — rusqlite 0.41.1 (`bundled`, WAL, synchronous=NORMAL) as the daemon's local file index
- **Source**: https://docs.rs/rusqlite/latest/rusqlite/ (0.41.1, MIT; `libsqlite3-sys` 0.39.1); GitHub rusqlite/rusqlite Cargo.toml (src: feature `bundled = ["libsqlite3-sys?/bundled", "modern_sqlite"]`; bundled `sqlite3.h` reports `SQLITE_VERSION "3.54.1"`); https://sqlite.org/wal.html ; https://sqlite.org/limits.html ; https://docs.rs/redb/latest/redb/ (4.4.0, MIT OR Apache-2.0). Local spike: rustc 1.97.1, x86_64 Linux, tmpfs. Accessed 2026-11-03.
- **Pinned mode**: We will use `rusqlite` 0.41.1 with feature `bundled`, one database file per account under the user data dir, `journal_mode=WAL`, `synchronous=NORMAL`, a `WITHOUT ROWID` table keyed by relative path (size, mtime, file id, BLAKE3 hash, sync state), one writer connection used from a blocking thread, with inputs = scan/watch results and sync results and outputs = per-path state lookups and diffs, for 100,000 rows on macOS, Windows and Linux. `sqlx` is not used.
- **Inputs in the example**: rusqlite `Connection`, `Transaction` ("Represents a transaction on a database connection"), `Savepoint`; WAL enabled by pragma (rusqlite has a `wal` feature-gated API for WAL hooks, not needed).
- **Outputs in the example**: not stated on the docs.rs front page beyond the API items; behaviour taken from SQLite docs and the local spike.
- **Project inputs**: 100,000 file rows; mostly batched inserts during scan, single-row updates during sync.
- **Project outputs required**: daemon total <=100 MB and <=1% idle CPU at 100k files (AC-15); state kept in the DB, not in RAM (00_ac_assessment).
- **Match assessment**: exact match for function; memory figure measured on Linux only.
- **If partial or mismatch**: SQLite WAL docs: "All processes using a database must be on the same host computer; WAL does not work over a network filesystem." (so a sync root or data dir on a network share is unsupported for the DB; place the DB in the local user data dir). "By default, a checkpoint runs automatically once the WAL reaches 1000 pages, which is about 4 MB." With `synchronous=NORMAL` "transactions may not be durable and could roll back after a power failure" (acceptable: the index is rebuildable by rescan, not the source of truth). Checkpoints cannot reset the WAL while a reader is open: the WAL "will grow without bound" with overlapping readers. Limits: max rows ~2e13, so 100,000 rows are irrelevant. Windows/macOS behaviour not measured. redb 4.4.0: "A simple, portable, high-performance, ACID, embedded key-value store"; docs front page says nothing on cache size, single-writer, or secondary indexes, so path-prefix queries and per-state indexes would be hand-built.
- **Measured here (not official docs; spike code in the session scratchpad, tmpfs so fsync cost is not represented)**: 100,000 rows (path ~48 B, 32-byte hash): rusqlite insert in one transaction 128 ms, full scan 13 ms, process VmHWM 7.1 MB, DB file 11.2 MB; redb insert 141 ms, full scan 7 ms, VmHWM 22.1 MB, file 16.9 MB. Both are far inside the 100 MB budget of AC-15 on Linux; SQLite cache and mmap settings were left at defaults.
- **Restriction/AC binding**:

| Restriction or AC id | Status | Evidence |
|---|---|---|
| AC-15 <=100 MB at 100k files | Pass on Linux (measured 7 MB for the store); Verify macOS/Windows and total daemon | spike above; watcher cache and HTTP buffers add to it |
| AC-15 first scan <=60 s | Pass for the DB part | 128 ms for 100k inserts in one transaction; enumeration cost is the walk (0.3 s warm per 00_ac_assessment) |
| Restrictions: "SQLite is excluded" for server storage | N/A (client-side only) | The exclusion text is about server writers on multiple hosts; client DB is single-host single-process |
| AC-18 echo suppression state | Pass | Table can hold file id, size, mtime, hash for suppression |

  Other restrictions and ACs: N/A.
- **Proposed status**: Selected: rusqlite `bundled` + WAL. redb is Rejected for v1 (no ad-hoc queries/indexes, 3x larger resident set in the spike, no benefit that the stated ACs need).

---

## MVE — reqwest 0.13.5 (rustls, HTTP/2 via ALPN) uploading fixed 2 MiB blocks on up to 4 concurrent streams
- **Source**: https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html (0.13.5); https://docs.rs/crate/reqwest/0.13.5/features ; https://docs.rs/reqwest/latest/reqwest/struct.Body.html ; https://docs.rs/hyper/latest/hyper/client/conn/http2/struct.Builder.html (hyper 1.12.0); https://docs.rs/h2/latest/h2/client/struct.Builder.html (h2 0.4.20); hyper `src/proto/h2/client.rs` and hyper-util `src/client/legacy/pool.rs` (src, main branch). Accessed 2026-11-03.
- **Pinned mode**: We will use `reqwest` 0.13.5 with default features (`default-tls` = rustls with the aws-lc-rs provider, `http2`) plus `stream`, one shared `Client` per account built with `tls_version_min(TLS 1.3)`, HTTP/2 negotiated by ALPN, explicit `http2_initial_stream_window_size` / `http2_initial_connection_window_size` (or `http2_adaptive_window(true)`), `http2_keep_alive_interval` set, and each 2 MiB block sent as its own request with a `Body` built from the in-memory block (`Bytes`), with at most 4 requests in flight (a tokio semaphore), with inputs = file blocks plus resume offsets and outputs = acknowledged blocks and a long-poll response, on macOS, Windows and Linux.
- **Inputs in the example**: `Body::wrap_stream` (needs `stream` feature) and `From<tokio::fs::File> for Body` (needs `stream` feature); `ClientBuilder::http2_*` methods (need `http2`).
- **Outputs in the example**: streamed request body; HTTP/2 flow-control settings applied.
- **Project inputs**: 2 MiB blocks, <=4 parallel lanes, TLS 1.3, HTTP/2, 20 ms RTT, 1 Gbps, resume re-sending <=10 MiB, held long-poll.
- **Project outputs required**: >=90% of link speed up to 1 Gbps (AC-2), interrupted upload resumes (AC-3), held long-poll for change notification.
- **Match assessment**: partial. HTTP/2, rustls, TLS-version floor and streaming bodies exist; "lanes" are streams on one TCP connection by default; resume is protocol logic of ours, not reqwest's.
- **If partial or mismatch**: Window defaults: reqwest docs say the stream and connection windows "may change internally to optimize for common uses" and give no number; h2 0.4.20 defaults are "65,535" for both; hyper's client sets 5 MB connection / 2 MB stream windows in source (`DEFAULT_CONN_WINDOW = 5mb`, `DEFAULT_STREAM_WINDOW = 2mb`, hyper main). The bandwidth-delay product at 1 Gbps and 20 ms is 2.5 MB (computed), so the 2 MB stream default alone is near the limit and 4 streams share a 5 MB connection window; set both explicitly and measure. Connections: hyper-util's pool treats an HTTP/2 connection as shared (`Reservation::Shared`) and de-duplicates concurrent connects for `Ver::Http2` (src), so 4 concurrent requests to one host share one TCP connection; a pool-level knob for N parallel connections is not documented (`pool_max_idle_per_host` only bounds idle sockets). If four TCP flows are wanted, four `Client`s are needed (Verify need by test). Frame size: "currently 16,384 but may change internally". Body: the Body docs do not describe retry/`try_clone` or Content-Length for stream bodies, so a retried upload must rebuild the body; with fixed 2 MiB blocks held in memory this is trivial and avoids file-streaming bodies entirely. Timeouts: `timeout` is a total-request timeout "until the response body has finished"; `read_timeout` "resets after a successful read" - use `read_timeout` plus HTTP/2 keep-alive for the held long-poll, not a total timeout (design note, from the quoted semantics). TLS: `tls_version_min` "By default, the TLS backend's own default is used", so TLS 1.3 must be requested explicitly. Rustls provider: default feature enables `__rustls-aws-lc-rs`; `rustls-no-provider` exists for choosing another (native build prerequisites of aws-lc-rs on Windows/macOS: Verify).
- **Restriction/AC binding**:

| Restriction or AC id | Status | Evidence |
|---|---|---|
| Sync protocol: HTTPS, TLS 1.3, HTTP/2, <=4 lanes, 2 MiB blocks | Pass | `tls_version_min`, `http2` feature, semaphore of 4 |
| AC-2 >=90% of 1 Gbps at <=20 ms RTT | Verify | Window settings above; one shared TCP connection by default; needs a LAN/netem test on each OS |
| AC-3 resume re-sends <=10 MiB | Pass by design | 4 x 2 MiB = 8 MiB in flight worst case (00_ac_assessment); reqwest has no resume feature, protocol is ours |
| Held long-poll (AC-5, AC-21) | Verify | `read_timeout` and keep-alive semantics quoted; no reqwest doc on very long holds |
| AC-15 memory <=100 MB | Verify | 4 x 2 MiB buffers = 8 MiB plus window memory; not measured |

  Other restrictions and ACs: N/A.
- **Proposed status**: Selected: reqwest 0.13.5 (rustls, h2) with explicit window settings; the "4 TCP connections vs 1 shared h2 connection" choice is left to the AC-2 spike.

---

## MVE — Packaging and signing per OS (facts and requirements only)
- **Source**: Apple "Notarizing macOS software before distribution" (https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution.md); Tauri 2 docs: https://v2.tauri.app/distribute/sign/macos/ , .../sign/windows/ , https://v2.tauri.app/distribute/ ; SMAppService pages quoted in the autostart card. Accessed 2026-11-03.
- **Pinned mode**: We will ship (macOS) a Developer ID-signed, hardened-runtime, notarized, stapled `.app`/`.dmg` containing the daemon, the Tauri settings app and the launchd agent plist; (Windows) a signed installer and signed executables; (Linux) packages built from the same binaries, with inputs = release builds from GitHub Actions and outputs = installable artifacts, on macOS, Windows and Linux. This card records requirements only; no tool choice is verified beyond what the sources state.
- **Inputs in the example**: Tauri macOS signing uses `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH` or `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`; Windows uses `certificateThumbprint`/`signCommand`.
- **Outputs in the example**: notarized Developer ID app that Gatekeeper accepts; signed Windows binaries.
- **Project inputs / outputs required**: installable, non-warning per-user daemon + settings app on all three OSes (Platform restrictions; AC-26 on Windows).
- **Match assessment**: partial (requirements are known; costs and CI secrets are operator/owner decisions).
- **Facts (macOS)**: notary service requires: code-signing of all executables, a "Developer ID" certificate ("Don't use a Mac Distribution, ad hoc, Apple Developer, or local development certificate"), Hardened Runtime for app and command line targets, a secure timestamp, no `com.apple.security.get-task-allow` = true, macOS 10.9+ SDK; deliverables: apps, disk images (UDIF), flat installer packages, non-app bundles; tooling `notarytool` and `stapler` ("the Apple notary service no longer accepts uploads from `altool` or Xcode 13 or earlier"); "all software built after June 1, 2019, and distributed with Developer ID must be notarized". Tauri docs: paid Apple Developer account "99$ per year"; free plan cannot notarize; "Only the Apple Developer `Account Holder` can create _Developer ID Application_ certificates"; ad-hoc signing "does not prevent MacOS from requiring users to" approve the app in Privacy & Security. LaunchAgent helper: Apple's sample keeps the launch agent plist and helper in a codesigned app bundle ("that neither the system nor a third party can modify without breaking the code signature") and SMAppService registration is "subject to user approval" in System Settings > General > Login Items. A separate helper executable inside the bundle is signed and notarized as part of that bundle; Apple's page does not give a per-helper rule beyond "all of the executables you distribute" (Verify with a real notarization run).
- **Facts (Windows)**: Tauri doc: OV guide "only applies to OV code signing certificates acquired before June 1st 2023"; Azure Key Vault via relic and `signCommand`; Azure Artifact Signing (formerly Azure Code Signing / Trusted Signing) via `artifact-signing-cli`; cross-compiling from Linux/macOS requires `signCommand`; "Since 2024, an EV Certificate no longer gives your app an immediate reputation with Microsoft SmartScreen", so a new signed release can still warn. The page states no hardware-key requirement and nothing about 2026 rules; eligibility and price of Artifact Signing were not verified from an official Microsoft page.
- **Facts (Linux)**: Tauri distribution page: "For Linux you can distribute your app using the Debian package, Snap, AppImage, Flatpak, RPM or Arch User Repository (AUR) formats." The settings app needs system WebKitGTK (libwebkit2gtk-4.1) and GTK3 (dialog plugin default feature); `tauri-plugin-single-instance` needs explicit D-Bus permissions under snap and flatpak ("it won't work as intended for snap or flatpak packages by default"). A systemd user unit is per-user, so it is enabled at first run or by user action, not by a system package script (design inference from unit location `~/.config/systemd/user`).
- **Restriction/AC binding**:

| Restriction or AC id | Status | Evidence |
|---|---|---|
| Platform: macOS LaunchAgent at login without UI | Verify | needs signed bundle + one-time Login Items approval |
| Platform: macOS file access granted once in system settings | Verify | not covered by the pages read; TCC behaviour for a signed LaunchAgent helper needs a test |
| AC-26 Windows logon task | Verify | installer must register the task (see autostart card) |
| CI: GitHub Actions builds | Verify | macOS runners and signing secrets not examined |

  Other restrictions and ACs: N/A.
- **Proposed status**: Needs user decision (Apple Developer Program account, Windows signing route and cost, which Linux formats to publish); requirements above are fixed facts.

---

## Findings that change the plan
1. **notify 8.2.1 does not signal Windows buffer overflow** (docs.rs 8.2.1 `windows.rs` has no Rescan/overflow path; `Flag::Rescan` for Windows is in 9.0.0-rc.x only). Plan a periodic safety rescan on every OS (walk+stat is ~0.3 s per 100k files warm) and decide whether to pin the 9.0.0-rc line once stable.
2. **notify-debouncer-full does not implement "unchanged for 2 s"**: each event is released when its own age reaches the timeout, later events do not extend it. AC-18's settle rule needs our own size+mtime stability check before upload. Own-write echo needs our file-id/size/mtime/hash table; FSEvents own-process marking exists only on the 9.0 rc line.
3. **tray-icon 0.26.1 already contains a GTK-free ksni backend**; the restrictions' "tray-icon, ksni" can be one dependency. Without a tray host (GNOME) the build succeeds and nothing shows, which satisfies AC-26 provided D-Bus failures are non-fatal. macOS needs the tray on the main thread with the NSApplication loop; the tokio runtime moves to worker threads.
4. **keyring has no verified durable headless-Linux fallback**: keyutils is in-memory and expires; db-keystore pushes the key problem to us. User decision needed. Also `v1` mode hides the Windows credential persistence setting (default Enterprise).
5. **AC-16 (1 s settings window) has no official figure** for Tauri 2; only a Tauri 1-era issue (default Ubuntu app USS 125 MB / PSS 185 MB) and a WebView2 cold-start caveat. Spike on the weakest Windows machine and a WebKitGTK VM before committing; Slint remains the stated fallback.
6. **reqwest/hyper window defaults**: h2 crate default 65,535 B; hyper client source default 2 MB stream / 5 MB connection; BDP at 1 Gbps x 20 ms is 2.5 MB. Set windows explicitly. Four "lanes" are four streams on one shared TCP connection by default.
7. **auto-launch 0.6.0 cannot create the Windows logon task** (it uses the Run key); write the three autostart mechanisms directly. SMAppService (macOS 13+) needs a signed bundle and user approval.
8. **Client store**: rusqlite bundled (SQLite 3.54.1) measured 7 MB resident for 100k rows (tmpfs, Linux) vs redb 22 MB; SQLite WAL must not sit on a network filesystem, so the index goes in the local user data dir even when the sync root is on a share.
9. **Version/licence notes**: notify is CC0, other notify crates MIT/Apache-2.0; interprocess 0BSD OR Apache-2.0; ksni Unlicense; rfd MIT; keyring MIT OR Apache-2.0; rusqlite MIT; redb MIT OR Apache-2.0. No copyleft found among the selected set (reqwest and tauri licences not re-verified in this pass). tauri-plugin-single-instance and tauri-plugin-dialog docs state a Rust 1.90 minimum while notify needs 1.88; use a toolchain at least 1.90 (spike built with rustc 1.97.1).

## Not verified (explicitly)
- Windows/macOS CPU and RAM of the whole daemon at 100k files (AC-15), Tauri cold start (AC-16), reqwest throughput (AC-2), removable-volume behaviour of notify, systemd user manager start-at-first-login wording, standard-user creation of a logon task without a prompt, TCC/Keychain prompts for a LaunchAgent, Windows code-signing price/eligibility.
