# Contract: transfer_service — the page-facing `TransferService` facade, the Worker protocol and the resume record

**Component**: 16_web_app
**Producer task**: E-26/c09 — `_docs/02_tasks/todo/B12-10_transfer_service_small_path.md` (facade, `enqueueUpload`, `cancelUpload`, `progress`); contributors E-26/c10 `B12-11_worker_large_upload.md` (Worker protocol, large path), E-26/c11 `B12-12_resume_edit_detection.md` (`resumeUpload`, resume record), E-26/c12 `B12-13_download_strategies.md` (`download`); keys assigned at Step 4
**Consumer tasks**: E-26/c08 (file browser row actions), E-26/c13 (public share page download), E-26/c17 (open action download offer), E-26/c19b (50 GiB runs), E-26/n01 (vault page, v1.2, needs the minor version below), pipeline `AZ-3671_pipeline_web_transfer`; resolved at Step 4
**Version**: 1.0.2
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

Pages hand files to one facade and read one progress store; they never talk to the Worker, the WASM module or the upload routes directly. Four tasks build the facade in parts, so its shape is fixed here, not in any one task spec.

Sources: `components/16_web_app/description.md` §2 (TransferService, Worker protocol), §4 (resume record), §5 (upload engine, downloads), §10; `epics/E-26_web_app.md` Interface Specification; `contracts/13_sync_core/sync_core_proto.md` (effective lanes, block size); `ui_mockups/web-files.html` (transfers panel).

## Shape

### Facade (TypeScript, behavioural)

```ts
type UploadJobId = string;                       // opaque, unique per tab
type NodeId = string;

interface UploadSource { file: File; relativePath?: string }   // relativePath for folder uploads

type UploadError =
  | 'InvalidName' | 'QuotaExceeded' | 'SizeTooLarge' | 'SessionCapReached'
  | 'FreeSpaceFloor' | 'ParentNotFound' | 'AccountNotActive' | 'TooManyFiles';
type ResumeError = 'FileUnreadable' | 'UploadSessionGone';
type DownloadFailure = 'HashMismatch' | 'Forbidden' | 'NotFound' | 'Network' | 'SaveFailed' | 'Cancelled';
// SaveFailed: the chosen place could not be written, or the browser has no capabilities set up. Cancelled: the person
// closed the save dialog; the row is `cancelled`, not `failed`.

type ResumeOutcome =
  | { kind: 'Resumed'; resentBytes: number }
  | { kind: 'Restarted'; reason: 'ChangedSinceLastTime' };

type UnverifiedReason =
  | 'OverBrowserVerifyLimit'      // Firefox/Safari, file above web.hash_verify_max_ff_safari
  | 'NoVerifiedPathInBrowser'     // no mechanism recorded as passing for this engine (Safari until the Mac run)
  | 'NotEnoughBrowserStorage'     // the private copy of the OPFS pull does not fit the browser's free space
  | 'NoServerHash';               // public link: the link answer carries no file hash (server gap)

type DownloadTarget =
  | { nodeId: NodeId; revisionId?: string }
  | { shareToken: string };

type DownloadOutcome =
  | { kind: 'VerifiedSaved' }
  | { kind: 'SavedUnverified'; reason: UnverifiedReason; serverHash: string | null }
  | { kind: 'Failed'; reason: DownloadFailure };

interface JobProgress {
  direction: 'upload' | 'download';
  name: string;
  bytesDone: number;
  bytesTotal: number;
  state: 'queued' | 'running' | 'committing' | 'done' | 'failed' | 'stopped' | 'cancelled';
  lanes: number;                                 // block lanes in use; 0 for the small path
  error?: UploadError | ResumeError | DownloadFailure;
  outcome?: DownloadOutcome;
}

interface TransferService {
  enqueueUpload(sources: UploadSource[], targetFolder: NodeId): Promise<Result<UploadJobId[], UploadError>>;
  resumeUpload(file: File, job: UploadJobId): Promise<Result<ResumeOutcome, ResumeError>>;
  cancelUpload(job: UploadJobId): Promise<void>;
  download(target: DownloadTarget): Promise<DownloadOutcome>;
  progress: Readable<Map<UploadJobId, JobProgress>>;  // Svelte store
}
```

`state: 'stopped'` is a large upload that ended with the tab or a suspended page and has a resume record. The state names are proposed here and confirmed by E-26/c09.

### Download strategies and labels (AZ-3942)

`download(target)` never rejects: it resolves with the outcome and leaves a `direction: 'download'` row in `progress` that carries the label. It first reads the metadata (`GET /nodes/{id}`, `GET /revisions/{id}` for a named revision, `GET /public-shares/{token}`): a folder, a tombstone or a 404 is `Failed{NotFound}`, a 403 or 401 is `Failed{Forbidden}`. The server's file hash is the `content_hash` of that answer.

| Browser capability | Strategy | Outcome |
|--------------------|----------|---------|
| Save dialog (`showSaveFilePicker`, Chromium), any size | the Worker reads ranged pulls of four 2 MiB blocks (`Range: bytes=a-b`), hashes each block with `sync-core-wasm`, writes it into the chosen file; the file is closed only when the roll-up equals the server hash | `VerifiedSaved`; on a mismatch the write is aborted, the file removed where the browser allows, `Failed{HashMismatch}` |
| No dialog, OPFS, and the engine where the OPFS pull was recorded as passing (Firefox, spike AZ-3931 candidate A2), size at most `web.hash_verify_max_ff_safari` (2 GiB build constant, `TransferDeps.hashVerifyMax` in tests) | same pull into a private OPFS file with the Worker's synchronous handle; after the roll-up matched the page hands the file to the browser's download manager (`blob:` link). Private files older than one hour are removed at the next such download | `VerifiedSaved`; mismatch: the OPFS file is deleted and nothing is handed over |
| Anything else (Firefox above the limit, Safari at any size until its Mac run is recorded, not enough OPFS room, a public link) | the browser's own download of the API URL (a plain link) | `SavedUnverified{reason, serverHash}`; the row says "Saved by the browser without a check (...)" and, when `serverHash` is set, "Compare its hash:" with the hash and a copy action |

Rules: no whole-file `Blob` or buffer on any path (one 2 MiB block at a time); a status or `Content-Length` is never verification; a body that ends early, a status other than 206 for a range (200 only when the range is the whole file) and a failed write end as `Network` or `SaveFailed`, never as a good file; `VerifiedSaved` is returned only after the roll-up of the block hashes equals the server's hash. `Cancelled` and `SaveFailed` apply to the dialog strategy. A page that is hidden or put away ends a running download as `Failed{Network}`. A public link has no server hash, so it is always `SavedUnverified{NoServerHash, null}`. The displayed and copied hash is the 64 hex digits of `content_hash`, without a prefix.

### Routing rule

A file of at most `ServerLimits.smallFileMax` bytes (2 MiB) takes the small path: `PUT /api/v1/staged-uploads/{client_key}`, then `POST /api/v1/commits:batch` (at most `batchMaxEntries` entries, one random UUIDv7 commit ID per batch, kept in memory and reused on every retry). A larger file goes to the module Worker. `uploadsEnabled: false` refuses every upload.

### Worker protocol (page to module Worker, structured clone; the page keeps no `File` after hand-off)

| Direction | Message | Fields |
|-----------|---------|--------|
| page → Worker | `start` | `file`, `parent`, `name`, `baseRevision?` |
| page → Worker | `resume` | `file`, `session` (from the resume record) |
| page → Worker | `cancel` | job id |
| Worker → page | `progress` | job id, `bytesDone`, `lanes` |
| Worker → page | `done` | job id, `revision` |
| Worker → page | `failed` | job id, typed error |
| page → Worker | `download` | job id, content route (`node` or `revision` id), `size`, server `hash`, sink (`picker` handle or `opfs` file name) |
| Worker → page | `downloadProgress` / `downloadDone` (`file` for an OPFS sink) / `downloadFailed` | job id, bytes done / the finished file / `DownloadFailure` |

### Resume record (IndexedDB, per signed-in account)

| Field | Type | Note |
|-------|------|------|
| `fileName` | string | match key |
| `size` | number | match key |
| `lastModified` | number | match key |
| `sessionId` | string | upload session id |
| `clientKey` | string | random UUIDv7 created with the session, reused on every resume |
| `blocklist` | block hashes | avoids re-hashing on resume; the server re-verifies every block |
| `parentId` | string | the folder the file goes to; the session read (`GET /uploads/{id}`) returns only the received-block list and expiry, so the commit's parent must come from here (added by AZ-3941) |

Stored per account: the key is `(accountId, clientKey)`, and a listing reads one account's records only. Written when the session is created; removed on commit, on cancel and when the server reports the session gone. Never holds a token, password or cookie. If IndexedDB is unavailable, uploads still complete, no record is kept and one notice says resume is not offered.

`resumeUpload(file, job)` applies to a job in state `stopped`: a file with the record's `fileName`, `size` and `lastModified` continues the session (the Worker `resume` message: read the session, send only missing blocks, no hash pass); any difference ends the old session (`DELETE /uploads/{id}`), drops the record and uploads the file from the start (`Restarted{ChangedSinceLastTime}`). `Resumed.resentBytes` is an upper bound for what the stopped run may have sent: the missing blocks before the last block the server holds, plus the first `lanes` missing blocks after it. A browser `File` is a snapshot: before commit the Worker re-reads `size` and `lastModified` and reads one byte; a change, or a refused read (the browser's answer to a file edited on disk), stops the job as `stopped` with the record kept, and selecting the file again restarts it. A page that is hidden or put away (`pagehide`) stops its large uploads the same way.

## Invariants

- Pages import only `TransferService` and the `ApiClient` facade; neither the Worker nor the WASM module is imported by a page.
- Hashing never runs on the main thread.
- At most `min(4, maxLanes)` block PUTs are in flight per upload; each request carries exactly one block; no streaming request bodies.
- At most `web.max_inflight_files` (200) `File` objects are queued in flight.
- A download whose hash does not match never ends as `VerifiedSaved`.
- Block hashes, roll-up, commit IDs and the resume diff come only from `sync-core-wasm`; there is no JavaScript implementation of these rules.

## Non-Goals

- Not covered: vault uploads and downloads (v1.2). A vault commit marker and the wrapped file key in `client_meta` need an optional `enqueueUpload` argument once E-30/c01 fixes the membership mechanism; that is a minor version (1.1.0) owned by E-26/n01.
- Not covered: change-feed sync (the browser does not sync), thumbnail polling (E-26/c15).

## Versioning Rules

- **Breaking**: a method, field or union member removed or renamed; a routing threshold no longer read from `ServerLimits`.
- **Non-breaking**: a new optional argument (vault context), a new `JobProgress` field, a new error member pages already show as a generic message.

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| boundary | files of 2 MiB − 1, 2 MiB, 2 MiB + 1 | first two staged and batch-committed; third through the Worker | IT-16-3 |
| retried-batch | network cut after the batch request | retry carries the same commit ID; no duplicate files | IT-16-3, FT-N-38 |
| lanes | 5 GiB upload, `max_lanes` 4 | at most 4 concurrent block PUTs, one block each | IT-16-4 |
| resume | tab closed at 50 %, same file re-selected | `Resumed{resentBytes <= 10 MiB}` | IT-16-5 |
| changed | re-selected file with another `lastModified` | `Restarted{ChangedSinceLastTime}` | IT-16-5 |
| corrupt | one corrupt block on the server copy | `Failed{HashMismatch}`, never `VerifiedSaved` | IT-16-8 |
| no-file-on-page | after `start` hand-off | page heap snapshot holds no `File` of the job | 16 §2 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B12) |
| 1.0.1 | 2026-11-21 | Resume record gains `parentId`; per-account keying, `resentBytes` meaning, edit-before-commit and `pagehide` behaviour written down | AZ-3941 |
| 1.0.2 | 2026-11-21 | `download` as built: strategy table, `UnverifiedReason`, `serverHash: string \| null`, `DownloadFailure` gains `SaveFailed` and `Cancelled`, Worker `download` messages | AZ-3942 |
