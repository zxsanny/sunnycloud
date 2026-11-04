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
