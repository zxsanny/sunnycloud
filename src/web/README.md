# sunnycloud web app

Svelte 5 single-page application (component 16, `_docs/02_document/components/16_web_app/`). It owns no server logic and holds no
tokens: the session is an HttpOnly cookie that the server sets. The build is static files (`dist/`) that Caddy serves
next to the `/api` proxy. Conventions: `_docs/02_document/components/16_web_app/svelte_conventions.md`.

## Commands

Bun 1.4.5 is the package manager and script runner (`packageManager`, `bunfig.toml` with `exact = true`; the lockfile is
`bun.lock`). Node 22 or newer must also be on PATH: the bins of vite, vitest, eslint and svelte-check start with
`#!/usr/bin/env node`, so `bun run <script>` runs them under Node. Forcing them under Bun (`bun --bun run test`) breaks
vitest's forks pool (`child.channel.unref is not a function`), and under the threads pool Bun's `Request` reports
`credentials: "include"`, which fails one client test; so the scripts do not use `--bun`. Bun itself runs
`bun scripts/gen-api.mjs`. Bun loads `.env` (no dotenv); see `.env.example`.

| Command                                 | What it does                                                                                                                                                                          |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `bun install --frozen-lockfile`         | Installs the exact versions of `bun.lock` (plain `bun install` updates it).                                                                                                           |
| `bun run dev`                           | Vite dev server on http://localhost:5173. `/api` is proxied to `VITE_PROXY_TARGET` (default `http://127.0.0.1:8080`, the server).                                                     |
| `bun run build`                         | Production build into `dist/` (gitignored).                                                                                                                                           |
| `bun run preview`                       | Serves the built `dist/` locally.                                                                                                                                                     |
| `bun run lint`                          | ESLint and `prettier --check`.                                                                                                                                                        |
| `bun run format` / `bun run format:fix` | `prettier --check` / `prettier --write`.                                                                                                                                              |
| `bun run typecheck`                     | `svelte-check`, failing on warnings (types for `.svelte` and `.ts`).                                                                                                                  |
| `bun run test`                          | Vitest, verbose. Includes a real production build and its CSP scan.                                                                                                                   |
| `bun run build:wasm`                    | Builds `sync-core-wasm` (wasm-pack) into `wasm/pkg` (gitignored). Needed before `build`, `typecheck` and `test`.                                                                      |
| `bun run check:large`                   | Manual: pushes a generated file (1 GiB; `LARGE_CHECK_MIB`) through the Worker code path against a fake server and prints the peak RSS. Runs under Node (it uses `fs.openAsBlob`).     |
| `bun run gen:api`                       | Regenerates the typed client sources from `openapi/openapi.json`; `bun run gen:api --refresh <path/to/sunnycloud>` first replaces the snapshot with the output of `<binary> openapi`. |
| `bun audit --audit-level=high`          | Known advisories of the lockfile.                                                                                                                                                     |
| `src/web/ci/web-ci.sh`                  | All of the above in CI order, plus client drift. The `web` job in `ci.yml` runs it.                                                                                                   |

The dev proxy does not rewrite `Origin` or `Host`: the server's CSRF check accepts a cookie request whose
`Sec-Fetch-Site` is `same-origin`, and the browser sends that for the dev server's own origin.

## The API client

`openapi/openapi.json` is the server's document, byte for byte (`sunnycloud openapi`). `scripts/gen-api.mjs` turns it into
`src/lib/api/generated/schema.d.ts` (types) and `problem-codes.ts` (the known error codes). Both are committed; CI
regenerates them and fails on any difference. After a server API change: build the server, run
`bun run gen:api --refresh <binary>`, commit the three files.

`src/lib/api` is the facade. `createApi()` returns one method per operation the app uses; each returns
`Result<T, ApiError>` and never throws for a server or network failure. It adds `X-Request-Id` (UUIDv7) and
`X-Sunnycloud-Client: web/<package version>`, uses same-origin credentials, adds no `Authorization` or CSRF header,
maps an unknown code or a non-JSON error body to `internal`, and stops sending after a 426. A method is added when a
page needs it. `src/app/handleApiError.ts` is the one place where 401, 426, 429, 507, 5xx and network failures become
notices (`isAppLevelError` says which failures are the app's; the rest go to the calling page).

## Dependencies

Every dependency is pinned exactly in `package.json`; `bun.lock` is committed; the registry is
registry.npmjs.org.

| Package                                                                                                        | Job                                                                                                                                                                           |
| -------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `svelte`                                                                                                       | The framework (runes). 5.57.3 is the version the packaging spike AZ-3930 measured.                                                                                            |
| `vite`, `@sveltejs/vite-plugin-svelte`                                                                         | Build, dev server, Svelte compiler integration (spike versions).                                                                                                              |
| `tailwindcss`, `@tailwindcss/vite`                                                                             | Utility classes mapped to the design tokens (DESIGN.md section 7).                                                                                                            |
| `packbin`                                                                                                      | The binary frame codec of the realtime channel (exact 0.5.1, the version the server's Rust crate packs identically). Brings `@noble/hashes` for frame sessions we do not use. |
| `openapi-fetch`                                                                                                | The small typed `fetch` client (about 6 kB) that the wrapper is built on.                                                                                                     |
| `openapi-typescript` (dev)                                                                                     | Generates the types from the OpenAPI document.                                                                                                                                |
| `@lucide/svelte`                                                                                               | The icon set of DESIGN.md (Lucide); imported per icon, so only used icons ship.                                                                                               |
| `@fontsource-variable/manrope`, `@fontsource/jetbrains-mono`                                                   | The two fonts as woff2 files served from our own origin (the CSP allows no font host). SIL OFL.                                                                               |
| `typescript` 5.9.3 (dev)                                                                                       | Types. Not 7: `svelte-check` and `openapi-typescript` accept only 5.x and 6.x as peers.                                                                                       |
| `svelte-check` (dev)                                                                                           | Type check of `.svelte` files.                                                                                                                                                |
| `eslint`, `@eslint/js`, `typescript-eslint`, `eslint-plugin-svelte`, `globals` (dev)                           | Lint, including the rules that keep the bundle inside the CSP.                                                                                                                |
| `prettier`, `prettier-plugin-svelte` (dev)                                                                     | Formatting.                                                                                                                                                                   |
| `vitest`, `jsdom`, `@testing-library/svelte`, `@testing-library/user-event`, `@testing-library/jest-dom` (dev) | Unit and component tests.                                                                                                                                                     |
| `@types/node` (dev)                                                                                            | Types for the tests and config that use `node:` modules.                                                                                                                      |

## Uploads: the transfer module (`src/lib/transfer`)

`transfer` (a `TransferService`) is the one facade pages use: `enqueueUpload(files, parentId)`, `cancelUpload(id)`,
`retryUpload(id)`, `dismiss(id)`, `onUploaded(listener)` and `jobs`, the reactive list of progress rows. It decides the
route by the server's `small_file_max` (read from `GET /server-info` on the first enqueue), keeps the queues (6 small
files staged at a time, one large file at a time), and refuses a selection above 100,000 files with `TooManyFiles`.

All hashing and every upload request run in a module Worker (`transfer.worker.ts`; the logic is `engine.ts`, which
Node can run too). The Worker loads `sync-core-wasm`: block hashes, the file roll-up, commit IDs and the `UploadPlanner`
all come from that one Rust implementation; there is no JavaScript copy of them.

- **Small file** (at most 2 MiB): hash, `PUT /staged-uploads/{key}` with the exact body and `X-Blake3`; staged files are
  committed by `POST /commits:batch` (at most `batch_max_entries`, a commit ID made by the WASM). A batch whose answer is
  lost is sent again with the same commit ID (3 attempts), so nothing is duplicated. `staged_cap_reached` commits what is
  staged, then stages the rest.
- **Large file**: a hash pass reads one 2 MiB slice at a time and keeps only the 32-byte hashes; `POST /uploads` with the
  file hash (`nothing_to_send` ends it); then the planner drives up to 4 lanes (the server's `lane_limit` if lower), one block
  per request, no streamed body; `POST /commits` with the blocklist. Block conflicts, hash refusals, transient errors
  and a lost session follow the planner's actions. A file is read, hashed, sent and dropped, so memory does not grow with the file.
- **Cancel** aborts the requests through an `AbortController`; a large upload also aborts its session. **Retry** starts the job
  again from the beginning with the same client key, so a session that is still alive is reused (`need_blocks` says what is missing).
- **Resume** (`resume.ts`, `service.svelte.ts`): when a large upload gets its session, the page stores a resume record in
  IndexedDB (`sunnycloud-transfer`, key `(accountId, clientKey)`: name, size, modified time, session id, client key,
  block hashes, folder; never a token or cookie). It is removed at commit, cancel and when the server says the session is
  gone. After the tab closed (or on `pagehide`) the job is `stopped`; the Files page lists the account's records
  (`loadStopped`) in a banner and as rows with "Select file". `resumeUpload(file, id)`: the same name, size and modified
  time continues the session (`resume` Worker message: `GET /uploads/{id}`, no hash pass, only the missing blocks; a hash
  the server refuses from the record restarts with a fresh hash pass); anything else ends the old session and uploads from the
  start. Before commit the Worker looks at the file again (size, modified time, one byte read): a changed file stops the job.
  IndexedDB unavailable: uploads work, no record is kept, one notice says so. Stale records of sessions that expired are
  removed when the file is selected (`UploadSessionGone`) or the row is cancelled.
- Typed errors per job: `InvalidName`, `QuotaExceeded`, `SizeTooLarge`, `SessionCapReached`, `FreeSpaceFloor`,
  `ParentNotFound`, `AccountNotActive`, `TooManyFiles`, plus `StagedCapReached`, `FileUnreadable`, `Network`, `Failed`.
  Faults the app reports itself (507, 429, 5xx, network) also reach the notice area.

## The file browser (`src/pages/FilesPage.svelte`, `src/lib/files`)

Routes `/` (the account's root) and `/folders/:id`. Every visit reads the folder, its first page (200 rows) and its
ancestors again; nothing is cached by path. `folderStore` holds the open folder. Rows merge by node id and feed position
(`upsert`: a lower `seq` is stale, a tombstone removes); the REST first load, the answer of a create and the pushed
realtime frames all go through it. Nothing in the page asks the server on a timer; a finished upload appears through its
Change frame (if the socket is down, when it reconnects and catches up).

Thumbnails: after each page one `POST /thumbnail-states` for the previewable files (image and video media types).
`ready` shows `<img src=".../thumbnail?size=small" loading="lazy" decoding="async">`; `pending` a skeleton with "Preview
pending"; `failed`, no job, and kinds with no preview an icon with "No preview". A video is its poster with a "Video" badge;
there is no `<video>` element. A click on an image opens a lightbox with `size=preview`; without a ready thumbnail only an
allowlisted image type (`renderMode`) falls back to the original. HTML, SVG, PDF and everything else get a download link. The
server's media type decides, never the file name. Download is a plain link to `/api/v1/nodes/{id}/content`.

## Realtime (`src/lib/realtime`, `src/app/realtime.ts`)

Live state arrives only as pushed frames over one WebSocket (`GET /api/v1/realtime`, contract `realtime.md`); REST is for
first load and writes. `frames.ts` has the packbin 0.5.1 schemes of every frame; `decodeServerFrame` returns a typed
result or a typed refusal (`empty`, `unknown_type`, `malformed`, `invalid_value`) and never throws on bytes from the network;
`u64` is `bigint`. packbin reads member names from the source text of the accessor lambdas, so `tests/bundle.test.ts`
builds `frames.ts` minified and runs every vector of `src/components/11_http_api/tests/fixtures/realtime_frames.txt`
through that build.

`RealtimeClient` (`client.svelte.ts`, a rune class with `state`: `idle`, `connecting`, `live`, `reconnecting`,
`signed-out`, `failed`) lives for as long as the file browser is open:

1. open the socket; wait for `Hello` (version 1, else close and show "reload to update");
2. the REST first load, read after Hello: `GET /changes/head` first, then the listing of the open folder (the head before the
   listing, so a change in between is re-sent by the subscription, never missed);
3. `Subscribe{epoch, seq}` at that head. The resume point moves only on a `Change` with `end` set.
4. `Ping` every `ping_interval_s` with a nonce; no `Pong` in 10 s, no `Hello` in 10 s, or 90 s without any message means a dead
   socket: leave it and reconnect. These timers and the reconnect backoff (1 s doubling to 30 s, up to 25 % jitter) are the
   only timers; none of them asks the server for data.
5. After a drop: `Subscribe` at the last resume point and one `POST /thumbnail-states` for the pending hashes. Close 4401
   goes to sign-in through the 401 path; 1002 stops and shows a reload notice; every other code reconnects.
6. `Reset`: the listing is read once over REST, then `Subscribe` at the head the frame names.

`src/app/realtime.ts` wires it: a `Change` becomes a row of the open folder when its parent is that folder (a tombstone or a
node that left the folder removes it) and sets the thumbnail state of its content hash; `ThumbState` goes to
`folderStore.setThumbState`. The app shell shows a chip: Live, Reconnecting or Offline.

## Memory check (`bun run check:large`)

Generated file of the shared-vector pattern, on disk, read slice by slice, uploaded through the same engine code the
Worker runs, against the fake upload server (which also hashes every block). Node 22.23.5, this host (zxstation).

| File  | Blocks | Baseline RSS | Peak RSS | Growth | Time   | Max lanes seen |
| ----- | ------ | ------------ | -------- | ------ | ------ | -------------- |
| 1 GiB | 512    | 175 MiB      | 239 MiB  | 64 MiB | 16.5 s | 4              |
| 4 GiB | 2048   | 132 MiB      | 207 MiB  | 75 MiB | 25.5 s | 4              |

Baseline is the process (vitest fork plus the loaded WASM) just before the upload. The growth does not follow the file
size: 4 times the data, the same 65 to 75 MiB. That is the four 2 MiB blocks in flight, the hashing copy in WASM
memory and the runtime's own churn. Node's `openAsBlob` reports size 0 from 4 GiB up, so the 4 GiB run reads the file with a small
file-handle wrapper of the same shape (`size`, `slice().arrayBuffer()`). This measures the engine in Node, not a
browser's heap (the 150 MB heap bound of AC-50 is for the Playwright runs of E-26/c19b).

A real-browser smoke (headless Chromium 1.65 against the built bundle under the production CSP, with a stub API) uploaded a
small file and 2 MiB + 1 byte: the Worker loaded the WASM, the two block hashes and the file hash equalled the shared
native vectors, `Content-Length` was exact, and the console showed no CSP violation or error. That script is not in the repo.

## The router (`src/lib/router`)

About 100 lines of our own instead of a router package: the app has a handful of fixed paths, no nested layouts and no
data loading by route. It holds the location in a reactive object, matches exact paths from a table (`src/app/routes.ts`),
renders the matching page or the not-found page, follows same-origin link clicks without a page load (links under
`/api/`, new-tab links and downloads stay with the browser), and handles back and forward. `:param` segments are supported (`/folders/:id`); `route.params` reads them from any page.

## Routes today

`/` and `/folders/:id` (the file browser), `/login`, `/invite#<token>` (the sign-in page in its invite state; the format follows the
placeholder in the mockup and is an open point of AZ-3936), `/setup`. Not built yet: shares, device approval, admin, account
page, vault, rename and move, folder uploads, resume across reloads, downloads with verification.
