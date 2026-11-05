# SPA scaffold: routing, generated `ApiClient`, error mapping, CSP and bundle checks

**Task**: AZ-3935_spa_scaffold
**Name**: SPA scaffold and API client
**Description**: Create the Svelte 5 web project with client routes for every page, the OpenAPI-generated `ApiClient` with its wrapper and one app-level error mapping, and the build, lint and CI checks that keep the bundle inside the strict CSP
**Complexity**: 5 points
**Dependencies**: AZ-3930_vite_wasm_spike, AZ-3761_ci_skeleton, AZ-3741_docker_compose_skeletons, AZ-3804_version_gate_server_info, AZ-3810_error_mapper, AZ-3827_route_registry_openapi, AZ-3652_initial_structure, AZ-4021_release_caddyfile_security_headers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3935
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c06

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — operations, credential transport, CSRF rule, produced by E-21/c11.
- `_docs/02_document/contracts/11_http_api/server_info.md` — `ServerInfo`, `X-Sunnycloud-Client`, 426 gate, produced by E-21/c06.
- `_docs/02_document/contracts/11_http_api/problem_details.md` — error body and codes, produced by E-21/c09.

## Problem

Every page needs one typed way to call the API and the same answer to the same error. Without a shared client, each page would build its own requests and messages, the headers the server checks (AC-58, AC-64) would drift, and a single inline script or foreign URL would break the strict CSP the deployment serves.

## Outcome

- The web project builds a static bundle that the api image ships and serves (SPA fallback, E-21/c11), with a client route for every page of this epic.
- Pages call only the `ApiClient` facade; every call returns a typed result and carries the request id and client version headers.
- The same `ApiError.code` always gives the same UI for 401, 426, 429, 507 and server errors, shown in one app-level notice area.
- Build, lint and the CI `web` job fail on an inline script, a non-self URL, `{@html}`, `document.cookie` or client drift from the OpenAPI document.

## Scope

### Included
- The Svelte 5 + Vite + TypeScript project; client routes for files, the public share page `/s/<token>`, device approval, sign-in, first run, account, admin (with `?tab=`), and the `/vault` route (content by E-26/c18); a not-found page for unknown routes.
- The `ApiClient` generated from the OpenAPI document and its wrapper per the contract below: same-origin relative URLs, `credentials: same-origin`, `X-Request-Id`, `X-Sunnycloud-Client: web/<version>`, no `Authorization` or CSRF header, problem bodies parsed into `ApiError`, network failures typed.
- Start-up reads: `ServerInfo` (its values win over the build defaults `web.lanes` 4 and `web.small_file_max` 2 MiB; a `block_size` other than 2 MiB disables upload with a message) and the session profile (role for admin visibility).
- One app-level error mapping and notice area (texts from `ui_mockups/web-files.html` state "Errors"): 401 to sign-in with the relative return path; 426 upgrade notice with the server text and a reload action, no retry, no further request; 429 live countdown; 507 quota notice with used and limit; 5xx request id with a copy action.
- Build checks (no inline script or handler, no non-self URL), lint rules banning `{@html}` and `document.cookie`, the CI `web` job (`svelte-check`, lint, unit tests, build, generated-client drift), and the web build stage of the api image.
- The CSP header test IT-16-9 against the release Caddyfile in the test stack.

### Excluded
- Page content (E-26/c07a to c18, n01), design tokens and shared components (E-26/n02), the transfer engine (E-26/c09 to c12), the Playwright fixtures (E-26/c19a).
- Emitting the CSP header itself: deployment-owned (16 §5); see Flagged concerns.

## Acceptance Criteria

**AC-1: Request headers and credentials (AC-58, IT-16-13 page half)**
Given the app signed in on the test stack
When it calls every operation it uses
Then each request goes to the page's own origin with `same-origin` credentials, carries `X-Request-Id` and `X-Sunnycloud-Client: web/<version>`, and carries no `Authorization` or CSRF-token header; the source has 0 references to `document.cookie`

**AC-2: Typed results**
Given problem bodies for every code the app maps, an unknown code, a 502 with an HTML body and a dropped connection
When the wrapper parses them
Then each yields an `ApiError` with status, code and the response request id, the unknown code and the HTML body give `internal`, the drop gives a network error, and no call throws past the wrapper

**AC-3: Error mapping by code (IT-16-14, AC-21, AC-64)**
Given the fault proxy answers 401, 426 `upgrade_required`, 429 with `Retry-After: 7`, 507 `quota_exceeded` and 500 with `X-Request-Id: r-9`
When the app makes the call
Then 401 lands on sign-in with the relative return path; 426 shows the upgrade notice with the server text and a reload action; 429 shows a 7-second countdown; 507 shows the quota message with used and limit from the quota view; 500 shows a danger notice containing `r-9` with a copy action and no raw JSON or stack trace in the DOM

**AC-4: 426 stops the client (FT-P-79 step 3)**
Given the server minimum version is raised above the web build
When the page reloads and the first call gets 426
Then the upgrade notice shows and the network log has 0 further upload or listing requests until the user reloads

**AC-5: Server errors carry the request id (FT-P-79 steps 4-5)**
Given the database is stopped
When the folder listing fails
Then the danger notice contains the `X-Request-Id` of the failed response exactly, the console has 0 uncaught errors, and after the database returns a reload shows the listing

**AC-6: `password_required` is not a sign-in**
Given a public share answers 401 `password_required`
When the app receives it
Then no redirect to sign-in happens and the code reaches the calling page

**AC-7: ServerInfo wins over build defaults**
Given `ServerInfo` with `max_lanes` 3, and in a second run `block_size` 4 MiB
When the app starts
Then the client exposes 3 lanes; in the second run uploads are disabled with a visible message and no upload request is sent

**AC-8: Bundle hygiene (IT-16-9 build half)**
Given seeded cases: an inline script, an inline event handler, a URL to another host, a `{@html}` and a `document.cookie` read
When the build and lint run
Then each seeded case fails the run, and the clean tree passes with 0 inline scripts and 0 non-self URLs in the output

**AC-9: CSP header and no violations (IT-16-9)**
Given the release Caddyfile and the api image in the test stack
When `/` is fetched and sign-in, browse, upload, download and share are exercised
Then the response carries every directive of the 16 §5 CSP, 0 CSP violations are reported, and 0 requests go to hosts other than the origin and the provider redirect returned by the API

**AC-10: Client drift gate and routes**
Given a changed operation in the OpenAPI document, and a reload on a deep client route
When the CI `web` job runs, and the page reloads
Then the job fails on client drift (passes once regenerated), and the reload serves the app at that route; an unknown route shows the not-found page

## Non-Functional Requirements

**Compatibility**
- Chromium, Firefox and Safari current releases; WebKit only for inline checks (environment.md).

**Reliability**
- The app works when browser storage is blocked (ST-16-2).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Problem-body parser over the code table, unknown code, HTML body, network error | Typed `ApiError` per case; nothing thrown |
| AC-3, AC-6 | Error mapping table | One presentation per code; `password_required` passed through |
| AC-7 | `ServerInfo` precedence over build defaults | Server values used; upload disabled on another `block_size` |
| AC-8 | Build scan and lint rules with seeded violations | Each seeded case fails |

## Blackbox Tests

Playwright against the real stack behind the release Caddyfile (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Signed-in session; network log in Chromium, Firefox, WebKit | IT-16-13 (page half); ST-16-3 same-origin control | Headers as stated; same-origin actions succeed | AC-58, NFT-SEC-09 |
| AC-3 | Fault proxy per status | IT-16-14 | Mapped UI per code | AC-21, AC-64 |
| AC-4, AC-5 | Raised minimum version; stopped database | FT-P-79 steps 3-5 | 0 requests after 426; request id equals the header | AC-64 |
| AC-9 | Release Caddyfile | IT-16-9 | CSP present; 0 violations; 0 foreign hosts | AC-47 |
| AC-10 | Deep route reload | SPA fallback | App served; unknown route not found | |

## Constraints

- Pages import only the `ApiClient` and `TransferService` facades (module-layout 16).
- No server-side rendering, no token handling in JavaScript, no third-party host (16 §1, §5).
- Dependency versions are pinned and match the rest of the repository; the generator version is pinned in the lockfile.

## Risks & Mitigation

**Risk 1: Generated client lags the server**
- *Risk*: a server change ships without a regenerated client.
- *Mitigation*: AC-10 drift gate in the PR job.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| No planned task emits the required CSP header: the Caddyfile structure in `deployment/containerization.md` has none, the API serves the SPA (E-21/c11) and its security headers in `api_v1_surface.md` list none. AC-9 (IT-16-9) cannot pass until the deployment owner adds the header | Deployment owner (E-00 / E-21) | open | High |
| The server issues its own request id and does not echo the client's `X-Request-Id` (`api_v1_surface.md` open point 10); the UI shows the response id, the sent id is client correlation only | E-02, E-09, E-21 owners | open | Low |
| The exact upgrade text is open (G-19); the notice shows the server's text | E-21, E-31/c03 | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/16_web_app/api_client.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
