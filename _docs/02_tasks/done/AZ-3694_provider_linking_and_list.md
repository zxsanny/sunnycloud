# Provider linking and the provider list with setup state

**Task**: AZ-3694_provider_linking_and_list
**Name**: Link provider and list providers
**Description**: Let a signed-in user attach another provider identity to their account (never by email), and tell clients which providers are enabled and whether the install still needs its first admin
**Complexity**: 2 points
**Dependencies**: AZ-3683_authenticator_scopes_cache, AZ-3684_provider_sign_in_callbacks, AZ-3685_invite_redemption_account_creation
**Component**: 01 identity
**Tracker**: AZ-3694
**Epic**: AZ-3646 (E-15)
**Ref**: E-15/c14

## Problem

A user who signed in with GitHub may want to sign in with an OIDC provider too. Linking must use the provider user id only: matching by email would let an attacker take over a victim's account through a provider that does not verify emails (AC-55). The sign-in pages and the first-run page also need the list of enabled providers and to know whether a setup token is still needed (AZ-3654 step 4).

## Outcome

- `SignInService::begin_link` and `complete_link` for a session principal: `Linked` or `IdentityTaken`.
- `SignInService::list_providers` returns the enabled providers and `setup_required`.

## Scope

### Included
- `begin_link(session principal, provider)`: same state handling as `begin` (hashed state bound to the browser, relative return path, host check), marked as a link attempt for that account.
- `complete_link`: same callback validation as `complete`; on success add an `identities` row for the account; an identity already bound to another account gives `IdentityTaken`; an unverified email is never copied to the identity row (data model §4.2).
- `list_providers()`: the enabled providers in configured order, and `setup_required` true exactly while no account exists.

### Excluded
- Callback validation internals (E-15/c04); the email rule at redemption (E-15/c05).
- The session-only check on the link route (E-21, checked by E-15/c13); the web pages (E-26/c07).
- Unlinking an identity (not in 01's interface).

## Acceptance Criteria

**AC-1: A second provider is linked to the signed-in account**
Given account A signed in with its GitHub identity
When A runs `begin_link` and `complete_link` for an OIDC identity that belongs to no account
Then the result is `Linked`, A has 2 identities, and a later sign-in with either identity reaches A

**AC-2: An identity of another account cannot be linked**
Given account B owns OIDC identity Y
When A tries to link Y
Then the result is `IdentityTaken` and neither account's identities change

**AC-3: Linking never uses an unverified email (AC-55)**
Given account V whose identity has a verified email `victim@example.test`, and mock-idp returning that address unverified for a new identity
When A links the new identity
Then the identity is linked only to A, no identity is added to V, and the identity row stores no email

**AC-4: Link callbacks reuse the sign-in state rules (AC-56)**
Given a link attempt
When its callback is replayed, sent with a foreign browser binding, or sent after `oauth_state_ttl`
Then each returns `UntrustedState` and nothing is linked

**AC-5: Provider list and setup state (AC-19)**
Given GitHub and one OIDC provider enabled, first with 0 accounts and then after the first account exists
When `list_providers` runs each time
Then it lists exactly the two enabled providers in configured order, with `setup_required` true the first time and false the second

## Non-Functional Requirements

**Reliability**
- A linking failure leaves the account's existing identities and sessions untouched.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Provider list from config | Order and set equal the enabled config entries |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | A with GitHub identity | Link OIDC identity | `Linked`; 2 identities; both sign in to A | FT-P-62 |
| AC-2 | B owns Y | A links Y | `IdentityTaken`; no change | |
| AC-3 | Victim email unverified | IT-01-6 link part | Linked only to A; no email stored | FT-N-22, NFT-SEC-02 |
| AC-4 | Link attempt | Replay, foreign binding, expired | `UntrustedState`; nothing linked | NFT-SEC-03 |
| AC-5 | 2 providers; 0 then 1 account | `list_providers` | Exact list; `setup_required` true then false | FT-P-62, FT-N-28 |

## Constraints

- Identity is `(provider, provider_user_id)`; linking never matches by email (RESTRICT-17, AC-55).

## Risks & Mitigation

**Risk 1: Account takeover through linking**
- *Risk*: a victim's session is tricked into completing an attacker's link.
- *Mitigation*: the link state is bound to the browser and the session's account; the route is session-only with CSRF checks (E-21).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `ProviderInfo` fields are not named in 01 (system gap G-15 also asks for the provider-list endpoint) | E-15 owner / E-31/c03 | open | Low |
| Linking an identity that is already linked to the same account: result not stated | E-15 owner | open | Low |
