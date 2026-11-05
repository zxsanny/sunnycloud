# Contract: transfer_service — the page-facing `TransferService` facade, the Worker protocol and the resume record

**Component**: 16_web_app
**Producer task**: E-26/c09 — `_docs/02_tasks/todo/B12-10_transfer_service_small_path.md` (facade, `enqueueUpload`, `cancelUpload`, `progress`); contributors E-26/c10 `B12-11_worker_large_upload.md` (Worker protocol, large path), E-26/c11 `B12-12_resume_edit_detection.md` (`resumeUpload`, resume record), E-26/c12 `B12-13_download_strategies.md` (`download`); keys assigned at Step 4
**Consumer tasks**: E-26/c08 (file browser row actions), E-26/c13 (public share page download), E-26/c17 (open action download offer), E-26/c19b (50 GiB runs), E-26/n01 (vault page, v1.2, needs the minor version below), pipeline `AZ-3671_pipeline_web_transfer`; resolved at Step 4
**Version**: 1.0.0
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
type DownloadFailure = 'HashMismatch' | 'Forbidden' | 'NotFound' | 'Network';

type ResumeOutcome =
  | { kind: 'Resumed'; resentBytes: number }
  | { kind: 'Restarted'; reason: 'ChangedSinceLastTime' };

type DownloadTarget =
  | { nodeId: NodeId; revisionId?: string }
  | { shareToken: string };

type DownloadOutcome =
  | { kind: 'VerifiedSaved' }
  | { kind: 'SavedUnverified'; reason: 'OverBrowserVerifyLimit' | string; serverHash: string }
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

### Resume record (IndexedDB, per signed-in account)

| Field | Type | Note |
|-------|------|------|
| `fileName` | string | match key |
| `size` | number | match key |
| `lastModified` | number | match key |
| `sessionId` | string | upload session id |
| `clientKey` | string | random UUIDv7 created with the session, reused on every resume |
| `blocklist` | block hashes | avoids re-hashing on resume; the server re-verifies every block |

Written when the session is created; removed on commit, on cancel and when the server reports the session gone. Never holds a token, password or cookie.

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
