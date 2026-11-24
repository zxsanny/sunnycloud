# mock-idp

A small OpenID Connect provider for acceptance runs: discovery, `/authorize` (auto-approves a configured user, PKCE S256
required), `/token` (RS256 id token, key generated at start), `/jwks`. It listens on 127.0.0.1 unless `MOCK_IDP_BIND_HOST` is set; only the compose test stack sets it (an isolated network, nothing published), because the mock approves anyone.

    MOCK_IDP_PORT=9000 MOCK_IDP_CLIENT_SECRET=... MOCK_IDP_REDIRECT_URIS=http://localhost:8080/api/v1/auth/providers/oidc/callback bun src/main.ts

Settings (environment): `MOCK_IDP_PORT` (default 9000, 0 = free), `MOCK_IDP_ISSUER` (default `http://127.0.0.1:<port>`),
`MOCK_IDP_CLIENT_ID`, `MOCK_IDP_CLIENT_SECRET` and `MOCK_IDP_REDIRECT_URIS` (comma separated, exact match; both required), `MOCK_IDP_USERS` (JSON, default `alice`
and `bob`), `MOCK_IDP_DEFAULT_USER` (default `alice`), `MOCK_IDP_ACCEPT_ANY_HINT=1` (a `login_hint` of letters, digits and dashes that names no configured user signs in as a new verified user of that name; the compose test stack sets it). The user of a sign-in is `login_hint`, else the one set by
`POST /__control/user {"user":"bob"}`; `POST /__control/fault {"fault":"wrong-nonce"|"wrong-audience"|"wrong-issuer"|"slow"}` (or `null`) scripts the next token exchange only: one claim of the id token is wrong, or the answer comes 12 s late (`slowMs`), else the default. Tests: `bun test`; types: `bun run typecheck`.
