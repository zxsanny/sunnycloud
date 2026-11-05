# sunnycloud

Sync files, photos, videos and all your files across your devices.

Install the API and workers on your own infrastructure.

Clients support web, Android, iOS and OS integrations: sync folders on macOS, Windows and Linux.

## Technologies

- **Server:** Rust API and background workers, OpenAPI-documented, installed with Docker Compose.
- **Web:** Svelte 5.
- **Mobile:** Flutter, with native iOS/Android code where needed.
- **Desktop:** native Rust daemon with tray icon and settings window for macOS, Windows and Linux.
- **Sign-in:** Google, Apple, Facebook, GitHub.
- **Encryption:** TLS, encryption at rest, opt-in end-to-end encrypted vault.
- **CI:** GitHub Actions.

## Repository layout

| Folder | Contents |
|---|---|
| `src/` | Cargo workspace root. Every `cargo` command runs here, because `rust-toolchain.toml` and `.cargo/config.toml` are found from the working directory. |
| `src/shared/` | Helper crates `sc_config`, `sc_error`, `sc_clock`, `sc_audit`, `sc_ids` (folder `ids_hashing`), `sc_names`, `sc_db`, `sc_ratelimit`, `sc_observability` |
| `src/components/` | Server components `01_identity` to `20_vault_service`, one library crate each |
| `src/contracts/`, `src/media-shared/` | Server port traits and shared types; media job and decoder protocol types |
| `src/server/`, `src/worker/`, `src/decoder/` | Binaries: `sunnycloud` (composition root and operator CLI), `sunnycloud-worker`, `decoder-runner` |
| `src/sync-core/`, `src/sync-core-wasm/` | Client core (native) and its WebAssembly adapter |
| `src/desktop/` | Desktop daemon, settings window and the IPC schema they share |
| `src/mobile_backup/`, `src/vault-core/` | Shared mobile uploader; vault cryptography (v1.2) |
| `src/migrations/`, `src/.sqlx/` | Reversible SQL migrations and committed sqlx offline query data |
| `deploy/` | Dockerfiles, Compose files and the PostgreSQL role init script |
| `scripts/` | CI scripts (`scripts/ci/`) that the CI jobs call; run them locally the same way |
| `_docs/` | Problem statement, architecture, task specs and implementation reports |

The full crate map and the allowed dependencies between crates are in `_docs/02_document/module-layout.md`. The web app (`src/web/`) and the Flutter app (`src/mobile/`) are created by their own epics.

## Local PR gate

Run these from `src/` before pushing. None of them uses a quiet flag.

```bash
cd src
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --locked --target wasm32-unknown-unknown -p sync-core-wasm -p sc_ids
cargo deny check advisories bans licenses sources
```

From the repository root:

```bash
sha256sum -c scripts/ci/licence.sha256
```

The migration and offline-query checks need a PostgreSQL 18 database; see `scripts/ci/README.md`.

## Licence

The project is licensed under AGPL-3.0-only (`LICENSE`). The App Store exception file `COPYING.iOS` is not in the repository yet: it waits for the owner's text.
