# Flow F2: Sign-in through OIDC or GitHub (Apple in v1.1)

Details, preconditions and errors: [system-flows.md](../../system-flows.md#flow-f2-sign-in-through-oidc-or-github)

## Sequence

```mermaid
sequenceDiagram
    participant Browser as Web app or system browser
    participant API
    participant PostgreSQL
    participant IdP as OIDC, GitHub or Apple

    Browser->>API: GET login for provider with relative return path and optional invite token
    API->>API: check return path is relative, Host matches public URL
    API->>PostgreSQL: save login state (state, nonce, PKCE verifier, provider, return path, invite ref, expiry)
    API-->>Browser: 302 to provider, redirect_uri built from configuration
    Browser->>IdP: user authenticates
    IdP-->>Browser: 302 to callback with code and state
    Browser->>API: GET callback
    API->>PostgreSQL: consume state atomically
    API->>IdP: token request with code and verifier, 10 s timeout
    IdP-->>API: id token or access token
    API->>API: validate issuer, audience, nonce, signature, expiry
    API->>PostgreSQL: find identity by provider and subject
    API->>PostgreSQL: create session, store SHA-256 of tokens
    API-->>Browser: session cookies, 302 to return path
    Note over Browser,API: mobile: the callback stores a hashed one-time login code and redirects to the app, which exchanges code and PKCE verifier for a session
```

## Callback decision flow

```mermaid
flowchart TD
    cb([Callback arrives]) --> hostOk{"Host matches public URL?"}
    hostOk -->|No| rejectHost([Reject])
    hostOk -->|Yes| stateOk{"State present, issued here, bound to this browser, unexpired, unused, same provider?"}
    stateOk -->|No| rejectState([Reject, no session])
    stateOk -->|Yes| exchange["Exchange code with verifier"]
    exchange --> exOk{"Provider accepted code and verifier within 10 s?"}
    exOk -->|No| rejectEx([Reject, sign-in fails clearly])
    exOk -->|Yes| tokOk{"Issuer, audience, nonce, signature valid?"}
    tokOk -->|No| rejectTok([Reject])
    tokOk -->|Yes| known{"Identity known?"}
    known -->|Yes| session["Create session"]
    known -->|No| hasInvite{"Login state carries invite or setup token?"}
    hasInvite -->|No| refuse([Refuse: invitation required, no account])
    hasInvite -->|Yes| allow{"Allowlist configured?"}
    allow -->|Yes| emailOk{"Provider email verified and listed?"}
    emailOk -->|No| refuse
    emailOk -->|Yes| redeem["Redeem token and create account in one transaction"]
    allow -->|No| redeem
    redeem --> session
    session --> done([302 to return path])
```
