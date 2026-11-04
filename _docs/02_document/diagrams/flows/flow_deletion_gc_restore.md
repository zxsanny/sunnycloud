# Flow F12: Deletion, blob GC queue, retention and operator restore

Details, preconditions and errors: [system-flows.md](../../system-flows.md#flow-f12-deletion-blob-gc-retention-and-operator-restore)

```mermaid
sequenceDiagram
    participant Client
    participant API
    participant PostgreSQL
    participant GC as GC loop (Janitor in the sunnycloud binary, no leader)
    participant Blobs as Blob volume
    participant Op as sunnycloud admin restore

    Client->>API: DELETE node (base revision) or overwrite commit
    API->>PostgreSQL: one transaction: tombstone or new revision, blob_gc row (due = now + retention), counter (05 port, last)
    loop every GC interval
        GC->>PostgreSQL: claim due rows by injectable clock, FOR UPDATE SKIP LOCKED or lease
        GC->>Blobs: unlink blob file (missing file is fine)
        GC->>PostgreSQL: remove_owner_rows (revision row whatever its state, FK clears current_revision_id), then blob and gc rows
        GC->>PostgreSQL: derived janitor: derived rows with no remaining revision, blobs queued with Due::Now (no retention)
        GC->>PostgreSQL: purge tombstones older than 90 days, move purge horizon
        GC->>PostgreSQL: expire upload sessions older than 24 h, release quota
        GC->>Blobs: sweep orphan blobs older than 48 h
    end
    Op->>PostgreSQL: restore by path and time: find revision, blob still present?
    alt blob present
        Op->>PostgreSQL: one transaction: re-point the existing revision row and re-parent it (blob_id is UNIQUE, no second revision), BlobRegistry reinstates the blob and clears the gc row, counter last
        Note over Op,PostgreSQL: placement Original (original folder exists) or Restored (Restored folder at the root)
        Note over Op,PostgreSQL: devices receive it as an ordinary change
    else already collected
        Op-->>Op: error: beyond retention
    end
```
