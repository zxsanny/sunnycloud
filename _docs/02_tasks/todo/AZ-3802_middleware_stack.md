# Middleware: panic catcher, client IP, request ID, metrics and log, security headers

**Task**: AZ-3802_middleware_stack
**Name**: Outer middleware layers
**Description**: The first five layers of the stack: a panic becomes a 500 problem body, the client IP honours trusted proxies only, every response gets a fresh UUIDv7 request id, requests are counted by route template with quiet 2xx logs, and every response carries the fixed security headers with CORS closed on `/api`
**Complexity**: 3 points
**Dependencies**: AZ-3801_crate_skeleton_accept_loop, AZ-3810_error_mapper, AZ-3827_route_registry_openapi, AZ-3764_sc_error, AZ-3786_sc_observability
**Component**: 11 http_api
**Tracker**: AZ-3802
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/c04

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/11_http_api/api_v1_surface.md` — security header and CORS rules (E-21/c11).
- `_docs/02_document/contracts/11_http_api/problem_details.md` — the 500 body (E-21/c09).
- `_docs/02_document/contracts/shared_observability/sc_observability.md` — log field schema and metrics registry (E-09/c01).
- `_docs/02_document/contracts/shared_error/sc_error.md` — `CorrelationId` (E-02/c01).

## Problem

A panic must not drop a connection that carries other streams. Abuse limits are counted per client IP (AC-20), so a client that forges `X-Forwarded-For` must not change its identity. Users report problems by request id, so every answer needs one. Every response, errors included, must carry `nosniff` and the other fixed headers (AC-47), and `/api` must not answer CORS preflights from foreign origins (AC-58).

## Outcome

- Layers 1 to 5 of 11 §5 run in order on the public and internal listeners.
- Request metrics `http_requests_total{route, method, status_class}` and `http_request_duration_seconds{route}` use the route template, never an id, token, name or share URL.
- Per-request info log lines only for non-2xx; share token path segments are redacted from logs.

## Scope

### Included
- Panic catcher: 500 problem body with request id, a panic counter, the connection kept.
- Client IP: the proxy-set `X-Forwarded-For` only when the peer is in `trusted_proxies`, otherwise the peer address; the value handed to handlers for ports that take a client IP.
- Request ID: a new UUIDv7 per request in `X-Request-Id`, the tracing span and error bodies.
- Metrics and log: the two HTTP families above and `body_limit_exceeded_total` registration (incremented by E-21/c05); quiet 2xx; redaction of `/public-shares/{token}` segments.
- Security headers: `Strict-Transport-Security: max-age=31536000`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `Cache-Control: no-store` by default (content routes set `private` through E-21/c10); no `Access-Control-*` headers on `/api`.

### Excluded
- Timeouts and body limits (E-21/c05), the version gate (E-21/c06), auth (E-21/c07), CSRF (E-21/c08).
- The public-share `Access-Control-Allow-Origin: *` answer (E-21/c14 with E-21/c10).

## Acceptance Criteria

**AC-1: Panic becomes a 500 and the connection survives**
Given a stub handler that panics
When a client sends the panicking request and then `GET /api/v1/server-info` on the same h2 connection
Then the first answer is 500 with a problem body and `request_id`, the second is 200 on the same connection, the panic counter rose by 1 and both carry `nosniff`

**AC-2: Client IP from trusted proxies only (AC-20)**
Given `trusted_proxies = 10.0.0.0/24` and a stub that records the client IP it receives
When a peer inside the range sends `X-Forwarded-For: 198.51.100.7`, a peer outside sends the same header, and an outside peer rotates `X-Forwarded-For` values over 20 requests
Then the stub sees `198.51.100.7` for the trusted peer and the peer address for the untrusted one in every request

**AC-3: One fresh request id per response**
Given 1,000 requests that end in 200, 404, 401, 413, 426, 500, OPTIONS and HEAD answers, some carrying a client `X-Request-Id`
When the responses are read
Then every response has `X-Request-Id` parsing as UUID version 7, all 1,000 are distinct, error bodies repeat the header value, and no client-supplied value is used as the id

**AC-4: Security headers on every response (AC-47, AC-58)**
Given answers with statuses 200, 204, 304, 400, 401, 403, 404, 405, 413, 416, 426, 429, 500 and 503, and `OPTIONS /api/v1/nodes` with `Origin: https://evil.test` and `Access-Control-Request-Method: POST`
When the headers are read
Then each has `Strict-Transport-Security: max-age=31536000`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer` and `Cache-Control: no-store`, and no response on `/api`, the preflight included, has any `Access-Control-*` header

**AC-5: Low-cardinality metrics and quiet logs**
Given 1,000 successful requests to `/api/v1/nodes/{id}` with distinct ids, then 10 failing ones, and one request to a public share path
When `/metrics` and the log are read
Then the ids produce one series per route template, no label holds an id, token, name or share URL, there are 0 per-request info lines for the 2xx requests and one per failing request, and the share token does not appear in the log

## Non-Functional Requirements

**Performance**
- The five layers together stay within PT-11-2's `server-info` p95 of 2 ms (failure 10 ms).

**Security**
- No request body, token, name or share URL reaches a log line (NFT-SEC-38).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Trusted-proxy decision for in-range, out-of-range and IPv6 peers | Header honoured only in range |
| AC-5 | Path redaction of `/public-shares/{token}` | Token replaced by the template |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Mode S, panicking stub | Panic catcher (IT-11-3) | 500 then 200 on one connection; counter +1 | AC-47 |
| AC-2 | Mode S, recording stub | Client IP (IT-11-15, IP part) | Trusted header only | AC-20, NFT-SEC-04 |
| AC-3 | Mode S, 1,000 requests | Request ID (IT-11-4) | UUIDv7, distinct, repeated in bodies | |
| AC-4 | Mode S, 14 statuses, preflight | Security headers (IT-11-5) | Headers present; no CORS on `/api` | AC-47, NFT-SEC-09, NFT-SEC-11 |
| AC-5 | Mode S, 1,010 requests | Metrics and logs (IT-11-17, metrics part) | One series per template; quiet 2xx | NFT-SEC-16, NFT-SEC-38 |

## Constraints

- Layer order is fixed by 11 §5 (outermost first).
- Metrics are registered through `sc_observability`; labels are enumerations only.

## Risks & Mitigation

**Risk 1: A label leaks personal data**
- *Risk*: a path parameter used as a label.
- *Mitigation*: the route template is the only path label; AC-5 checks 1,000 ids.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Request id source: 11 §5 and IT-11-4 issue a fresh id and never echo a client value, while `sc_error::CorrelationId::from_request` keeps a valid incoming value, `sc_observability` AC-3 expects a caller-set id and 16's `ApiClient` sends one. This task follows 11 | E-02, E-09, E-21 owners | open | Medium |

## Contract

This task implements the security-header and CORS rows of the contract at `_docs/02_document/contracts/11_http_api/api_v1_surface.md` (producer E-21/c11).
Consumers MUST read that file — not this task spec — to discover the interface.
