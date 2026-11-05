# Release Caddyfile with the web app's CSP and security headers

**Task**: AZ-4021_release_caddyfile_security_headers
**Name**: Release Caddyfile and SPA security headers
**Description**: Write the release Caddyfile as `deployment/containerization.md` describes it (TLS 1.3, HTTP/1.1 and HTTP/2 only, raised body limit, no buffering, h2c to the API) and make it emit the web app's Content-Security-Policy of 16 §5 on the SPA's responses, so IT-16-9 and NFT-SEC-11 can pass
**Complexity**: 3 points
**Dependencies**: AZ-3652_initial_structure, AZ-3741_docker_compose_skeletons, AZ-3742_test_harness_skeleton, AZ-3799_s1_caddy_h2_spike, AZ-3802_middleware_stack, AZ-3812_content_header_mapper, AZ-3827_route_registry_openapi; notes: E-31/c05 (test epic, Step 9: the test stack's caddy service mounts this file)
**Component**: Bootstrap (deployment edge config; repository path `deploy/`, owner E-00 per module-layout "Repository-level paths")
**Tracker**: AZ-4021
**Epic**: AZ-3621 (E-00)
**Ref**: E-00/n01

## Dependencies

### Document Dependencies
- `_docs/02_document/deployment/containerization.md` — "Caddy and Postgres 18" and "Caddyfile (structure)".
- `_docs/02_document/components/16_web_app/description.md` §5 — "CSP requirement (edge config owned by deployment)".
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` and `content_serving_headers.md` — the headers the API already sets (HSTS, `nosniff`, `Referrer-Policy`, sandbox CSP on attachments).

## Problem

16 §5 says the web app's CSP is edge config owned by deployment, and IT-16-9 reads the header "through Caddy, from the release compose file". No task emits it: the Caddyfile structure in `containerization.md` has no CSP line, the API serves the SPA (E-21/c11) and its fixed security headers (E-21/c04) have no CSP. No task writes the release Caddyfile at all either: AZ-3741 builds only the development compose file and excludes "the release `compose.yml` and Caddyfile", while the test stack, the S1 spike and the Mode R suites all run "the release Caddyfile, not a test copy" (`tests/environment.md`). Without it, IT-16-9 (AZ-3935 AC-9), NFT-SEC-11 and the frame check of NFT-SEC-07 cannot pass.

`deploy/` belongs to E-00, and later changes go "by the epic whose AC needs them, one task at a time" (module-layout). The AC that needs this file is the web app's, but the file is E-00's, so the task sits in E-00.

## Outcome

- One release Caddyfile, used unchanged by the release bundle and by the test stack.
- TLS 1.3 only, HTTP/1.1 and HTTP/2 to clients (no HTTP/3), port 80 redirected to HTTPS, request body limit at least 64 GB, no response buffering, h2c to the API's public listener, `X-Forwarded-For` replaced (not appended), no route to `:8081` or `:9090`.
- Every response of the SPA (the index, the SPA fallback paths, the built assets) carries the 16 §5 CSP: `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`, with no third-party host.
- HSTS (one year) and `X-Content-Type-Options: nosniff` stay the API's (E-21/c04, "emitted by the API, not Caddy, so operators on nginx, Traefik or a tunnel get it too"); this task checks they reach the browser through Caddy and does not set them a second time.

## Scope

### Included
- The release Caddyfile per the documented structure, plus the CSP header for SPA responses.
- Checks through the test stack that the documented edge properties and headers hold.

### Excluded
- The bundle rules (no inline script, no non-self URL) and the CSP-violation run: E-26/c06 (AZ-3935, IT-16-9 bundle half).
- HSTS, `nosniff`, `Referrer-Policy`, `Cache-Control` and the sandbox CSP on attachments: the API (AZ-3802, AZ-3812).
- The v1.2 vault page additions (`object-src 'none'`, see Flagged concerns).
- The release `compose.yml`, `release.yml` packaging and the install guide's nginx, Traefik and Cloudflare Tunnel notes (see Flagged concerns).
- The development Caddyfile (AZ-3741).

## Acceptance Criteria

**AC-1: The SPA carries the 16 §5 CSP (IT-16-9 header half, AC-47)**
Given the test stack started with the release Caddyfile and the api image with an embedded web bundle (the SPA serving of E-21/c11; a minimal bundle is enough before E-26/c06 lands)
When `GET /`, an SPA fallback path such as `/files/x`, the public share page path and one built asset are fetched through Caddy
Then each response has exactly one `Content-Security-Policy` header with `default-src 'self'`, `script-src 'self' 'wasm-unsafe-eval'`, `style-src 'self'`, `img-src 'self' blob:`, `connect-src 'self'`, `frame-ancestors 'none'`, `base-uri 'none'` and `form-action 'self'`, and it names no host other than `'self'`

**AC-2: Attachments keep their own CSP (AC-47)**
Given an HTML file and a PNG uploaded by a test user
When the HTML file is downloaded and the PNG is fetched inline through Caddy
Then the HTML download still has `Content-Security-Policy: sandbox`, `Content-Disposition: attachment` and `nosniff` as the API sets them, and neither response gains a second CSP from the edge

**AC-3: The approval page cannot be framed (NFT-SEC-07 step 6)**
Given a page on another origin that frames the device approval page
When a browser loads it
Then the frame is blocked by `frame-ancestors 'none'`

**AC-4: Transport properties at the edge (NFT-SEC-11 steps 1, 2 and 4)**
Given the test stack with the release Caddyfile
When a TLS scan runs against port 443, a plain request goes to port 80, and 20 HTTPS responses of different routes (SPA, `/api`, error statuses) are inspected
Then TLS 1.3 is negotiated, TLS 1.1 and 1.0 are refused, ALPN offers `h2`, no `Alt-Svc` advertises `h3`; port 80 answers with a redirect to HTTPS and serves no API content; every HTTPS response from the API carries `Strict-Transport-Security` with `max-age` at least 31,536,000 and `nosniff`, each exactly once

**AC-5: Large bodies pass unbuffered (FT-P-69 step 3, FT-P-05 step 3)**
Given the test stack with the release Caddyfile
When a 2 MiB block is uploaded and a 5 GiB file is uploaded and downloaded through Caddy
Then no 413 is returned, the proxy container's disk grows by at most 100 MB, and the connection to the client is HTTP/2 over TLS 1.3

**AC-6: Only the public listener is routed**
Given the test stack
When requests through Caddy try the internal worker routes and the ops paths (`/api/v1/worker-jobs/status`, `/metrics`, `/readyz` on `:8081` and `:9090` addresses)
Then none reaches those listeners, and the API sees the client address only in a replaced `X-Forwarded-For`

## Non-Functional Requirements

**Compatibility**
- Caddy is the pinned `caddy:2@sha256:<pin>` image; the directives used must exist in that version.

**Reliability**
- The same file is mounted in the test stack and shipped in the release bundle; a test copy is not allowed (`tests/environment.md`).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | `caddy validate` (or `caddy adapt`) on the release file | Valid; the CSP value equals the 16 §5 string |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Test stack, release Caddyfile, web bundle | IT-16-9 (header half) | One CSP with all 16 §5 directives on SPA responses | AC-47 |
| AC-2 | HTML and PNG uploads | IT-11-14 expectations through Caddy (NFT-SEC-19 steps 2-3) | Sandbox CSP kept; no second CSP | AC-47 |
| AC-3 | Cross-origin framing page | NFT-SEC-07 step 6 | Frame blocked | AC-57 |
| AC-4 | Test stack | NFT-SEC-11 steps 1, 2, 4 | TLS 1.3, no h3, redirect, HSTS once | AC-54, RESTRICT-11 |
| AC-5 | 2 MiB block, 5 GiB file | FT-P-69 step 3; FT-P-05 step 3 | No 413; disk growth <= 100 MB; h2 over TLS 1.3 | AC-22, AC-2 |
| AC-6 | Test stack | Requests to internal and ops paths through Caddy | Not routed; `X-Forwarded-For` replaced | AC-20 |

## Constraints

- The CSP string is the one in 16 §5; a change goes through 16 §5 first.
- No second copy of HSTS or `nosniff` at the edge: the API stays the single source so other proxies get them too.

## Risks & Mitigation

**Risk 1: `'wasm-unsafe-eval'` under review**
- *Risk*: the AC-62 review may refuse `'wasm-unsafe-eval'` (16 open question 3).
- *Mitigation*: the policy string lives in one place in this file; a change is one line and AC-1 re-checks it.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| With the CSP at the edge, operators who bring nginx, Traefik or a tunnel get no CSP unless the install guide carries the line; `containerization.md` moved HSTS into the API for exactly this reason. Owner may prefer the API's SPA serving (E-21/c11) to emit the CSP instead; this task follows 16 §5 and IT-16-9 ("deployment-owned") | Owner (deployment) | open | Medium |
| 16 §5 says `img-src 'self' blob:` "where needed"; this task puts it in the one SPA policy because thumbnails and previews use blob URLs. A per-route policy is not described | E-26 owner | open | Low |
| The v1.2 vault page needs `object-src 'none'` in addition (NFT-SEC-42 step 1, AZ-3954 AC-8); 16 §5 does not list it, so this v1.0 task leaves it out. The v1.2 change to this file has no task yet | Owner / E-26 | open | Medium |
| No task writes the release `compose.yml` either (AZ-3741 excludes it; FT-P-69 and IT-16-9 name it); this task writes only the Caddyfile | Owner / E-00 | open | Medium |
| Whether the SPA policy should also be set on `/api` JSON responses is not stated; this task sets it on SPA responses only and leaves the API's own headers unchanged (AC-2) | E-21 / deployment owner | open | Low |
| AZ-3935 (its High flagged row on the missing CSP emitter; its AC-9 is IT-16-9) and AZ-3950 should depend on this task once it has a key; this task does not depend on AZ-3935, so there is no cycle | Parent (Step 4) | open | Low |
| The test compose file's caddy service (E-31/c05, not yet a task) must mount this file; AZ-3742's skeleton has no caddy service | E-31 owner | open | Low |
