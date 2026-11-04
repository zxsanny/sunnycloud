# Flow F14: Database restore, epoch bump, resync

Details, preconditions and errors: [system-flows.md](../../system-flows.md#flow-f14-database-restore-epoch-bump-and-resync)

```mermaid
sequenceDiagram
    participant Op as Operator
    participant CLI as sunnycloud admin (12 operator_cli)
    participant API
    participant PostgreSQL
    participant Device

    Op->>PostgreSQL: stop stack, restore database dump
    Op->>CLI: admin epoch-bump after restore (master key mounted)
    CLI->>PostgreSQL: set a new epoch for every account feed
    CLI->>CLI: write the epoch to the .epoch marker on the blob volume
    Note over CLI,API: if the bump is skipped, serve refuses to start (database epoch below the marker) and prints admin epoch-bump; doctor reports Fail
    CLI->>PostgreSQL: reconcile: blobs without rows, rows without blobs
    CLI-->>Op: report
    Op->>API: start API (checks schema version and epoch)
    Device->>API: GET changes with old cursor
    API-->>Device: resync_required
    Device->>API: GET head cursor, then paged tree listing
    API-->>Device: new cursor and full tree
    Device->>Device: compare with LocalIndex
    alt only local
        Device->>API: upload (F4 or F5)
    else only on server
        Device->>API: download (F6)
    else both and equal
        Device->>Device: relink, 0 bytes
    else both and different
        Device->>API: upload local as conflict copy, download server version
    end
    Note over Device: no delete is sent except through the mass-delete guard
    Device->>API: resume long-poll from the new cursor
```
