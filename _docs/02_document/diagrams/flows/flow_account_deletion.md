# Flow F13: Account deletion

Details, preconditions and errors: [system-flows.md](../../system-flows.md#flow-f13-account-deletion)

```mermaid
sequenceDiagram
    participant User as User (web session)
    participant API
    participant PostgreSQL
    participant Purger as DeletionPurger (07) driving AccountLifecycleHook
    participant Blobs as Blob volume

    User->>API: DELETE account with confirmation (session only)
    API->>PostgreSQL: transaction 1: state = deleting, revoke sessions and device tokens (commits refused from now on)
    API-->>User: 202 accepted, session ends
    loop erase_batch on every hook until all report remaining = 0, short transactions
        Purger->>PostgreSQL: 02 revisions and nodes (blobs to blob_gc with Due::Now), 03 uploads, receipts, reservations, 05 feed row, 06 shares, 07a quota, 08 jobs and derived, 20 vaults
        Purger->>Blobs: GC unlinks originals, thumbnails, transcodes
    end
    Purger->>PostgreSQL: 01 finalize_deletion: identities, sessions, device tokens, device grants, login codes, account row; write audit entry (IDs only)
    Note over User,Purger: complete within 15 min of the request
```
