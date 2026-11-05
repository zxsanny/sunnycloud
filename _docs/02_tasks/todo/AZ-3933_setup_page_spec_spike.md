# Spike: specify the setup-token first-run page (route, fields, states)

**Task**: AZ-3933_setup_page_spec_spike
**Name**: First-run page specification spike
**Description**: Write the route, trigger, fields, states and messages of the first-run page from the approved sign-in mockup and the identity rules, and unblock IT-16-12
**Complexity**: 1 point
**Dependencies**: None
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3933
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c04

## Problem

AC-23 needs a first-run page that creates the first admin with the one-time setup token, but the 16 description lists the page without route, fields or messages, so IT-16-12 is BLOCKED (`tests/spec-gaps.md` row IT-16-12). The owner approved the mockup `ui_mockups/web-sign-in.html` on 2026-11-04 (states "First run", "Token error", "First run: sign in"); it fixes the fields and texts. Still unwritten: the route, how the page learns it is needed, and when the token is checked (`ui_mockups/DESIGN.md` §9 #9).

## Outcome

- `components/16_web_app/description.md` §5 gains a "First-run page" paragraph: route, trigger, fields, the two steps, messages and what is sent to the server.
- IT-16-12 in `components/16_web_app/tests.md` is rewritten with input and expected UI per state; the IT-16-12 row of `tests/spec-gaps.md` is marked resolved with the date.
- E-26/c07a builds the page from that paragraph.

## Scope

### Included
- Reading 01 (`list_providers` returns `setup_required`; `begin` takes an optional setup token in the body), the mockup states and FT-P-64, FT-P-78, FT-N-20, FT-N-21.
- Writing the paragraph and the rewritten IT-16-12.

### Excluded
- Building the page (E-26/c07a); any server change (E-15).

## Acceptance Criteria

**AC-1: Route and trigger recorded**
Given the providers answer with `setup_required = true`
When the paragraph is written
Then it names the page route, states that every signed-out route leads to it while `setup_required` is true, and that it cannot be reached once it is false

**AC-2: States, texts and outcomes recorded**
Given the three mockup states
When the paragraph is written
Then it lists each state with its texts and controls, the neutral invalid-token message, and the API outcome that leads to each state (`InvitationRequired` after the provider round trip, or an earlier check if E-15 offers one)

**AC-3: Token transport recorded**
Given 01 §3 ("body carries invite/setup token, never in a URL")
When the paragraph is written
Then it states that the setup token is sent only in the body of the login attempt and is never written to browser storage or a URL

**AC-4: IT-16-12 unblocked**
Given the paragraph
When the test spec is updated
Then IT-16-12 has input and expected UI for each state, and the spec-gaps row IT-16-12 is marked resolved with the date

## Non-Functional Requirements

None beyond the criteria.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-4 | Review of the rewritten IT-16-12 against the mockup states | Every state has an input and an expected result |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1..AC-4 | Approved mockup, 01 spec, FT-P-64, FT-P-78 | Document review by the E-26 owner | Paragraph and IT-16-12 consistent with 01 and the mockup | AC-23 |

## Constraints

- No new server behaviour: where 01 is silent, the paragraph says so and the item stays open.

## Risks & Mitigation

**Risk 1: Late token check**
- *Risk*: if the token is checked only at the callback, the "Token error" state appears after the provider round trip.
- *Mitigation*: the paragraph states that order explicitly so the page text matches it.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| When the setup token is checked (at the login attempt or only at the callback) is not specified (`ui_mockups/DESIGN.md` §9 #9) | E-15 owner | open | Low |
