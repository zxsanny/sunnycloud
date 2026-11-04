# Problem

sunnycloud is a self-hosted file, photo and video sync service for hundreds of users per installation. An operator installs the API and background workers on their own infrastructure. Users sync all their files across devices.

Clients:
- Web app (Svelte 5).
- Android and iOS apps (Flutter, with platform-specific code where needed), including automatic camera-roll backup.
- Desktop sync clients for macOS, Windows and Linux: a native Rust daemon that watches chosen folders and syncs them both ways, with a tray icon and a small settings window. Settings use the operating system's native dialogs for choosing folders; the app draws no file browser of its own.

Server behavior:
- Photos get thumbnails; videos are transcoded (streaming is out of scope for now).
- Files can be shared by link.
- Folders marked as a vault are end-to-end encrypted; all other data is encrypted at rest on the server.
- Sync model is simple: last write wins, no version history.
- Users sign in with Google, Apple, Facebook or GitHub.

Server components (API, workers, desktop agents) are written in Rust for maximum performance. How files are stored on the server and which database holds the metadata are decided by research.
