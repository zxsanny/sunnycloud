# Contract: api_client — the web app's typed API client, its wrapper and the app-level error mapping

**Component**: 16_web_app
**Producer task**: E-26/c06 — `_docs/02_tasks/todo/B12-06_spa_scaffold.md` (keys assigned at Step 4)
**Consumer tasks**: E-26/n02, c07a, c07b, c08, c09, c10, c11, c12, c13, c14, c15, c16, n01 (every page that calls the API and the transfer engine); pipeline `AZ-3671_pipeline_web_transfer` (AC-5 import check); resolved at Step 4
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

Every page and the transfer engine talk to the server through one generated client and one thin wrapper, so requests carry the same headers and every error reaches the user through the same mapping. Pages import only this facade and `TransferService` (`transfer_service.md`); no page calls `fetch` itself.

Sources: `components/16_web_app/description.md` §2, §5 (error handling, CSRF), §10, §11; `contracts/11_http_api/api_v1_surface.md`, `server_info.md`, `problem_details.md`; `ui_mockups/DESIGN.md` §6 (mapped API errors); `ui_mockups/web-files.html` state "Errors".

## Shape

### Generated operations

Generated from the OpenAPI 3 document of 11 (`GET /api/v1/openapi.json`, same bytes as `sunnycloud openapi`). The generated code is committed; the CI `web` job regenerates it and fails on any difference. Operation names follow the document; the web app uses the groups listed in 16 §2 (server info, auth, sessions, devices, account, admin, tree, uploads, commits, content, shares, public shares, thumbnails).

### Wrapper (TypeScript, behavioural)

```ts
type Result<T, E> = { ok: true; value: T } | { ok: false; error: E };

interface ApiError {
  kind: 'http' | 'network';          // 'network' = no HTTP answer (drop, DNS, abort)
  status: number | null;             // HTTP status, null for 'network'
  code: string;                      // stable snake_case code; unknown or missing -> 'internal'
  title: string | null;              // server text, shown only where this contract says so
  requestId: string | null;          // from the response X-Request-Id (equals body request_id)
  retryAfterSeconds: number | null;  // 429 and 503
  minVersion: string | null;         // 426 only
  updateUrl: string | null;          // 426 only
  needBlocks: number[] | null;       // need_blocks only
}

interface ServerLimits {             // from GET /api/v1/server-info; these values win over build defaults
  blockSize: number;                 // uploads disabled when != 2097152
  maxLanes: number;                  // effective lanes = min(web.lanes 4, maxLanes)
  smallFileMax: number;              // default 2 MiB
  batchMaxEntries: number;
  maxStagedPerAccount: number;
  uploadsEnabled: boolean;           // false when blockSize != 2 MiB
}
```

| Rule | Behaviour |
|------|-----------|
| Origin | relative `/api/v1/...` URLs only; never another host |
| Credentials | `credentials: 'same-origin'` on every request; the browser supplies `Origin` and `Sec-Fetch-Site` |
| Headers added | `X-Request-Id` (fresh UUIDv7 per request), `X-Sunnycloud-Client: web/<build version>` |
| Headers never added | `Authorization`, any CSRF-token header, any cookie access from JS |
| Results | every call returns `Result<T, ApiError>`; nothing throws past the wrapper except programming errors |
| Error body | `application/problem+json` parsed into `ApiError`; a non-JSON error body gives `code: 'internal'` with the response `X-Request-Id` |
| Start-up | `ServerLimits` read once per page load; the session profile (`GET /api/v1/accounts/me`) read once for role-dependent UI |

### App-level error mapping (one place; pages may handle a code first only where listed)

| Code / status | Presentation | Handled first by |
|---------------|--------------|------------------|
| 401 (any code except `password_required`) | go to the sign-in route with the current relative path as `return`; notice "Your session ended…" | none |
| 401 `password_required` | none at app level | public share page (E-26/c13) |
| 426 `upgrade_required` | blocking notice with the server text and a reload action; no automatic retry; the client sends no further API request until reload | none |
| 429 (any code) | "Try again in N seconds" with a live countdown from `retryAfterSeconds`; the triggering control stays disabled until 0 | public share page and device approval page show it in place |
| 507 `quota_exceeded` | "Storage quota reached" notice with used and limit from `GET /api/v1/accounts/me/quota` | transfers panel adds a per-job line (E-26/c09) |
| 5xx, `internal`, `network` | danger notice with `requestId` in mono and a copy action (network: retry hint, no id); never the raw body | none |
| any other code | returned to the calling page, which shows it in place | the calling page |

## Invariants

- Pages and the Worker reach the server only through this wrapper; a static check finds no other `fetch`, `XMLHttpRequest` or `EventSource` use in non-test code.
- The same code always produces the same presentation on every page.
- No token, password, share password, setup token or invite token is written to `localStorage`, `sessionStorage`, IndexedDB or Cache Storage by the wrapper, and none is logged to the console.
- A 426 stops the client: after it, 0 further API requests until the page reloads.

## Non-Goals

- Not covered: request and response bodies of each operation (the OpenAPI document of 11 is the authority).
- Not covered: transfers (`transfer_service.md`), the visual style of notices (E-26/n02).
- Not covered: the request id the server logs; 11 issues its own (api_v1_surface open point 10), and the UI shows the response `X-Request-Id`.

## Versioning Rules

- **Breaking**: a field of `ApiError` or `ServerLimits` removed or retyped; a mapping row changed to another presentation.
- **Non-breaking**: a new optional field; a new code row whose presentation is "returned to the calling page".

## Test Cases

| Case | Input | Expected | Notes |
|------|-------|----------|-------|
| headers | any state-changing call in Chromium, Firefox, WebKit | same origin, `same-origin` credentials, `X-Request-Id`, `X-Sunnycloud-Client: web/<v>`; no `Authorization`, no CSRF header | IT-16-13 |
| parse-known | 429 problem body with `retry_after: 7` | `ApiError{status 429, code rate_limited, retryAfterSeconds 7}` | IT-16-14 |
| parse-unknown | problem body with code `brand_new_code` | `code: 'internal'` | problem_details "unknown codes" |
| non-json | 502 with an HTML body and `X-Request-Id: r-9` | `code: 'internal'`, `requestId: 'r-9'` | IT-16-14 |
| upgrade-stops | 426 on a listing | upgrade notice; 0 further requests (network log) | FT-P-79 step 3 |
| password-required | 401 `password_required` on a public share | no sign-in redirect | E-26/c13 |
| block-size | `block_size` 4 MiB in server info | `uploadsEnabled: false`; 0 upload requests | IT-16-4 |

## Change Log

| Version | Date | Change | Author |
|---------|------|--------|--------|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B12) |
