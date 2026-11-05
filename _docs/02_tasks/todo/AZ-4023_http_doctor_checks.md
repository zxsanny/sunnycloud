# HTTP doctor checks: public URL, TLS 1.3, HTTP/2 and proxy probe

**Task**: AZ-4023_http_doctor_checks
**Name**: HTTP doctor checks
**Description**: `DoctorCheck`s owned by 11: the public URL is `https`, resolves, negotiates TLS 1.3 and HTTP/2; the trusted-proxy list is not empty; and a 3 MiB probe body to the public URL is not rejected by the proxy (the nginx 1 MB default)
**Complexity**: 3 points
**Dependencies**: AZ-4021_release_caddyfile_security_headers, AZ-3768_contracts_part1, AZ-3801_crate_skeleton_accept_loop, AZ-3802_middleware_stack, AZ-3803_body_limits_timeouts, AZ-3807_doctor_aggregation; notes: new task `N-01_release_caddyfile_security_headers` the parent resolves its key
**Component**: 11 http_api
**Tracker**: AZ-4023
**Epic**: AZ-3644 (E-21)
**Ref**: E-21/n01

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part1_registries_identity_admin_quota.md` — `DoctorCheck`, `DoctorTag`, `DoctorContext`, `Finding`.
- `_docs/02_document/contracts/11_http_api/http_server.md` — `public_url` and trusted-proxy settings.
- `_docs/02_document/deployment/observability.md` "`sunnycloud doctor`" table — rows "Public URL" and "Proxy".

## Problem

AC-22's install check ends with `doctor` exit 0 "(proxy, TLS 1.3, HTTP/2, key, volume, providers)" (`deployment_procedures.md` step 9), and the go-live checklist says "TLS 1.3 and HTTP/2 confirmed by `doctor`". 12 §6 assigns the "public URL/TLS 1.3/HTTP/2/proxy body probe" to 11, but no E-21 task implements it (AZ-3809 flags the gap). The most common operator mistake the proxy probe catches is a reverse proxy with nginx's 1 MB default body limit, which answers 413 to every 2 MiB block (`containerization.md`).

## Outcome

- A public URL check: Fail-level findings when `public_url` is not `https`, its name does not resolve, the TLS handshake to it is not TLS 1.3, or the connection does not negotiate HTTP/2; Ok otherwise.
- A proxy check: Warn when the trusted-proxy list is empty ("all clients share one address"); a non-Ok finding when a 3 MiB probe body sent to the public URL is rejected by the proxy (413); Ok otherwise.
- Both run from inside the api container (`docker compose exec api sunnycloud doctor`) against the public URL, so they test the real path through the operator's proxy; both are read-only, print no secret, and are registered with the composition root's `DoctorCheck` list (E-22/c03).

## Scope

### Included
- The two `DoctorCheck`s in 11 and their unit tests; a blackbox run through `doctor` with the release Caddyfile and with a deliberately misconfigured proxy.

### Excluded
- The aggregation and exit codes (AZ-3807); configuration validation of `public_url` at start (helper 01, AZ-3790); the identity checks (E-15/n01); the release Caddyfile itself (E-00/n01).

## Acceptance Criteria

**AC-1: Healthy install passes (AC-22, AT-12-1 step 3, FT-P-69)**
Given the test stack with the release Caddyfile, a resolvable `public_url` with a test CA trusted by the api container, and `SUNNYCLOUD_TRUSTED_PROXIES` set to Caddy's address
When `doctor` runs
Then the public URL and proxy findings are Ok, and they do not stop `doctor` from exiting 0

**AC-2: Public URL faults (observability "Public URL")**
Given, in turn, `public_url` with `http://` (with `allow_plain_http_dev` on so the stack starts), a name that does not resolve, a proxy that offers TLS 1.2 at most, and a proxy that offers HTTP/1.1 only
When `doctor` runs for each
Then each gives a Fail-level public URL finding whose message names the fault (`https`, name resolution, TLS version, HTTP/2) and whose remedy names what to change

**AC-3: Proxy faults (observability "Proxy")**
Given, in turn, an empty trusted-proxy list, and an nginx proxy in front of the API with the default 1 MB `client_max_body_size`
When `doctor` runs for each
Then the first gives a Warn that all clients share one address, and the second gives a non-Ok proxy finding that names the 413 and the proxy body limit as the remedy

**AC-4: The probe changes nothing (IT-12-15)**
Given the healthy stack
When `doctor` runs 3 times
Then no table row is inserted, updated or deleted by the probe, no upload session or staged file exists afterwards, and the probe request is answered without a 2xx side effect

**AC-5: No secrets in output (NFT-SEC-38)**
Given known session, device and worker tokens in the environment
When `doctor` and `doctor --json` run
Then none appears in the public URL or proxy findings

## Non-Functional Requirements

**Performance**
- Each probe is bounded (connect and request within 10 s); the whole `doctor` stays within its budget (target 60 s, failure 180 s, PT-12-1).

**Compatibility**
- The checks work with Caddy (release file) and with any proxy in front of the public URL; they test what the operator's proxy actually does.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Mapping of scheme, resolution, TLS version and ALPN results to findings | Fail with message and remedy per fault |
| AC-3 | Mapping of trusted-proxy config and probe status | Warn when empty; non-Ok on 413 |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Test stack, release Caddyfile | `doctor` (AT-12-1 step 3) | Findings Ok | AC-22, FT-P-69 |
| AC-2 | `http://` URL; unresolvable name; TLS 1.2 proxy; HTTP/1.1 proxy | `doctor` (no ID yet) | Fail per fault | NFT-SEC-11 |
| AC-3 | Empty trusted proxies; nginx with 1 MB limit | `doctor` (no ID yet) | Warn; non-Ok naming 413 | AC-20 |
| AC-4 | Healthy stack | 3 `doctor` runs (IT-12-15 read-only part) | 0 row changes; no upload left behind | |
| AC-5 | Known tokens | `doctor`, `doctor --json` (IT-12-15) | 0 secrets | NFT-SEC-38 |

## Runtime Completeness

- **Named capability**: the "public URL/TLS 1.3/HTTP/2/proxy body probe" of 12 §6 (owner 11).
- **Production code that must exist**: real DNS resolution, a real TLS handshake and ALPN negotiation to the public URL, and a real 3 MiB request through the operator's proxy.
- **Allowed external stubs**: none in blackbox tests; recording fakes for the result mapping in unit tests.
- **Unacceptable substitutes**: a check that reads only configuration, a probe sent to the API's own listener instead of the public URL, a check that always returns Ok.

## Constraints

- The probe goes to the public URL, never to `:8080` directly, so it measures the proxy.
- 11 stays delivery-only: the checks live beside the server code and use no layer-3 component.

## Risks & Mitigation

**Risk 1: Probe answered by the API's own body limit**
- *Risk*: a 3 MiB body to a route with a small body class gets the API's own 413, which looks like a proxy fault.
- *Mitigation*: see the first flagged concern; the probe target must accept a 3 MiB body, or the API's answer must be told apart from the proxy's.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The route the 3 MiB probe uses is not stated, and most routes have a smaller body class (`AZ-3803`); how the check tells the proxy's 413 from the API's own limit, without side effects, needs a decision (for example a route of the large body class answering without creating state) | E-21 owner | open | Medium |
| The levels are only partly stated ("Fails or warns when"; empty trusted proxies is warn); this task uses Fail for public URL faults and leaves the 413 level as non-Ok | E-21 / E-22 owners | open | Low |
| Inside the api container the public URL may resolve to the host's public address (hairpin NAT) or not resolve at all on some networks; the install guide does not say how the check reaches the public URL from inside | E-21 owner / deployment | open | Medium |
| The tags of both checks are not stated; they need the running proxy, so they are not `pre-flight` | E-22 owner | open | Low |
| The registration list of the composition root (AZ-3789) names owner checks generically; the parent should make sure these checks are in it once they have a key | Parent (Step 4) | open | Low |
