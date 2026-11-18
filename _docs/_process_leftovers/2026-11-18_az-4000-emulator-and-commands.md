# Leftover: AZ-4000 emulator tests and the commands of later tasks

kind: test (NOT-RUN(NO-SERVICE)) and downstream work
recorded: 2026-11-18
blocked action: (1) the blackbox emulator tests IT-17-19 (a completed run with a real upload sends a snapshot; settings survive an app restart; the no-token check against a real sign-in): they need the emulator harness (AZ-4003) and the real server stack (E-31); (2) the native side and Dart methods of `signIn`, `signOut` (AZ-3992), `startLargeBackup` (AZ-3995), `requestPhotoAccess` (AZ-3993/AZ-3999), `requestBatteryExemption`, `exportDiagnostics` (AZ-4001): they answer `notImplemented` today
reason: the owners of those commands do not exist yet; the bridge exposes `StatusChannel.publish()` because `BackupCore` has no status-change callback (a Rust surface change would add one)

Replay: each owning task adds its command and calls `publish()` on status changes; the emulator tests run once AZ-4003 and the E-31 stack exist; delete this file when all are done.
