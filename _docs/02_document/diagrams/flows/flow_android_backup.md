# Flow F16: Android camera backup

Details, preconditions and errors: [system-flows.md](../../system-flows.md#flow-f16-android-camera-backup)

```mermaid
sequenceDiagram
    participant MS as MediaStore
    participant WM as WorkManager worker (Kotlin)
    participant Core as Shared Rust uploader
    participant UI as Flutter UI
    participant DB as Checkpoint DB
    participant API

    MS->>WM: content-URI trigger (or periodic safety job, or app open, or user-initiated transfer job)
    WM->>Core: run_backup() through UniFFI (Kotlin owns the uploader handle)
    Core->>DB: read last MediaStore generation
    Core->>WM: list assets with generation above checkpoint
    WM-->>Core: asset ids, sizes, mtimes (only visible assets under limited access; UI shows N photos visible, tap to allow more)
    loop each asset, bounded memory
        Core->>DB: asset known and done?
        alt unknown
            Core->>Core: BLAKE3 hash, save hash in DB
            Core->>API: upload via F4 or F5 (deterministic commit id)
            API-->>Core: committed revision
            Core->>DB: mark done
        else same asset id, new hash
            Core->>API: upload as new file linked to the original
        end
    end
    Core->>DB: store new generation
    WM-->>UI: status over Method/EventChannel
    Note over Core,API: access token 15 min, rotating refresh token: single-flight refresh, new refresh token persisted before use
    Note over Core,API: after reinstall: rebuild checkpoint from server listing, 0 bytes re-sent
    Note over WM: process killed: next run resumes from checkpoint
```
