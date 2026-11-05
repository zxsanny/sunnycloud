# Rules screen: per-pair patterns with the reported line marked and grammar help

**Task**: AZ-3898_rules_screen
**Name**: Rules screen
**Description**: The Rules tab: one multi-line pattern field per pair, saved with `update_pair`; a `RulesInvalid{line}` answer marks only that line and keeps Save disabled until the line is edited; the text is sent unchanged; the help lists the six grammar elements
**Complexity**: 3 points
**Dependencies**: AZ-3888_stub_daemon_ui_driver, AZ-3890_daemon_client, AZ-3895_settings_app_shell
**Component**: 15 desktop_settings
**Tracker**: AZ-3898
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c09

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `update_pair`, `PairPatch{rules_text}`, `RulesInvalid{line}` (1-based), `PairView.rules_text` (E-24/c05).
- `_docs/02_document/contracts/14_desktop_daemon/daemon_control.md` — `update_pair` replaces the rules and changes nothing on `RulesInvalid` (E-24/c19).

## Problem

Rules are gitignore-style patterns relative to the pair root (AC-66). The daemon validates them; the window must show exactly the line it reports, keep the user from saving while that error stands, and send the text unchanged, including a trailing newline and non-ASCII characters. Users also need the grammar at hand.

## Outcome

- The selected pair's rules load from the daemon; Save sends one `update_pair` with the text as typed.
- A `RulesInvalid{line}` answer marks that line only; Save stays disabled until that line is edited.
- The help lists `*`, `**`, `?`, leading `/`, trailing `/` and `!`.

## Scope

### Included
- A pair selector and one multi-line field with line numbers (mockup Rules tab), loaded from `PairView.rules_text`.
- Save rules: one `update_pair{pair_id, patch: {rules_text}}`; success shows the "Applied" mark.
- On `RulesInvalid{line}`: the line number and a flat band on that line (no opacity, DESIGN.md §6), the message "Line N is not a valid pattern", and Save disabled with "Fix line N to save" until the marked line is edited.
- The help text for the six grammar elements, as listed in 15 §5 and the mockup.
- Accessible names for the pair selector, the field, Save and the error message.

### Excluded
- Rule matching (14's RuleMatcher, E-24/c09).
- Rules on the add-pair form (E-25/c08 sends an empty rules text).

## Acceptance Criteria

**AC-1: Only the reported line is marked (AC-66, IT-15-4)**
Given a pair whose rules text is `*.log`, `build/`, `[`, `!keep.log`, `/x`, `a/**/b?` and a stub answering `RulesInvalid{line: 3}`
When the user saves
Then line 3 alone carries the marker and the message names line 3, and no other line is marked

**AC-2: Save disabled while the error stands**
Given the marker on line 3
When the user edits another line, then edits line 3
Then Save stays disabled after the first edit and is enabled after the second, and the next Save sends `update_pair` again

**AC-3: Text sent unchanged (IT-15-4)**
Given a rules text with a trailing newline and non-ASCII characters
When the user saves
Then the stub's record of `rules_text` is byte-equal to the text in the field

**AC-4: Grammar help (IT-15-4)**
Given the Rules tab
When the help is read through the accessibility tree
Then it lists `*`, `**`, `?`, leading `/`, trailing `/` and `!`, each with its meaning

**AC-5: Applied and no hidden state**
Given valid rules and a stub that accepts `update_pair`
When the user saves, closes the window and opens it again
Then exactly one `update_pair` was sent, the "Applied" mark appeared, and the field shows the daemon's `rules_text` after reopening

## Non-Functional Requirements

**Reliability**
- A Save that gets no answer within the client's 5 s timeout keeps the field text and shows the "did not answer" error (E-25/c05 timeout); no change is reported as applied.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Marker placement for `line` 1, the last line and a line beyond the text | Correct line, or a message without a marker for an out-of-range line |
| AC-2 | Save enable state across edits | Disabled until the marked line changes |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Stub `RulesInvalid{line: 3}` | Save | Line 3 only marked | IT-15-4, AT-15-1 |
| AC-2 | Marker on line 3 | Edit other line, then line 3 | Save disabled, then enabled | IT-15-4 |
| AC-3 | Pattern fixture with trailing newline and non-ASCII | Save | Byte-equal record | IT-15-4 |
| AC-4 | Rules tab | Read help | Six elements listed | IT-15-4 |
| AC-5 | Stub accepts | Save, reopen | One call; Applied; daemon text shown | FT-P-60 (UI part), NFT-PERF-26 |

## Constraints

- The window never parses or rewrites patterns; the daemon is the only validator (15 §11).

## Risks & Mitigation

**Risk 1: A line number that does not match what the user sees**
- *Risk*: the window and the daemon count lines differently (trailing newline, CRLF).
- *Mitigation*: AC-3 sends the text unchanged and AC-1 uses the 1-based line of `ipc_schema.md`; E-25/c14 runs AT-15-1 against the real daemon.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| 15 and the epic say "live" validation, but `DaemonControl` has no validate-only method and `update_pair` applies at once; this task validates on Save. A dry-run method would be an `ipc_schema` change (E-24) | E-24 / E-25 owners | open | Medium |
| The mockup's "Always excluded" line lists 14's default excludes, which no `DaemonControl` method returns; it is shown only if `ipc_schema` exposes the list | E-24 / E-25 owners | open | Low |
