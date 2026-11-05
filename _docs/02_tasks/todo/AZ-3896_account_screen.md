# Account screen: server address, sign-in with a code, sign-out

**Task**: AZ-3896_account_screen
**Name**: Account screen
**Description**: The Account tab: the server address applied with `set_server` and its errors beside the field, device sign-in that shows the verification URL and user code (never a password or token), the login state, and sign-out with the server-copy warning
**Complexity**: 3 points
**Dependencies**: AZ-3888_stub_daemon_ui_driver, AZ-3890_daemon_client, AZ-3895_settings_app_shell
**Component**: 15 desktop_settings
**Tracker**: AZ-3896
**Epic**: AZ-3628 (E-25)
**Ref**: E-25/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/14_desktop_daemon/ipc_schema.md` — `set_server`, `login_start`, `login_status`, `logout`, `LoginPrompt`, `LoginState`, `LogoutOutcome`, `AccountView` (E-24/c05).
- `_docs/02_document/contracts/14_desktop_daemon/daemon_control.md` — their effects and errors (E-24/c19).

## Problem

The user points the daemon at a server and signs it in from this window, but the window must never handle a token: sign-in is the device grant in the system browser, and the window only shows the URL and the code (F3, AC-48). Plain-HTTP addresses must be refused at the address field (AC-54). Errors must sit beside the control that caused them.

## Outcome

- The address is applied by one `set_server` call; its four errors appear beside the field.
- Sign-in shows the URL and the user code, follows the login state to a final state, and offers a retry after Denied or Expired.
- Sign-out calls `logout` and warns when the server copy still needs revoking.

## Scope

### Included
- Server address field with Apply, the helper "Must start with https://", and the "Applied" mark after success (mockup Account tab).
- Errors beside the address field: `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired{message}` (the message shown as given), `SignedIn` ("Sign out to change the server").
- Sign-in: `login_start` with the device name; the verification URL and the user code shown large, in a mono font and copyable; the expiry time; `ServerNotSet` beside the sign-in button.
- Following `LoginState` (`Pending`, `Approved`, `Denied`, `Expired`, `SignedOut`): Approved shows the signed-in account and sign-out; Denied and Expired show a message and a retry.
- Sign-out: `logout`; with `server_revoked = false` a warning tells the user to remove the device on the web.
- Accessible names and roles for every control of the screen.

### Excluded
- Opening the browser (the daemon does it at `login_start`, `daemon_control.md`); the mockup's "Open browser" button (flagged).
- The device grant itself (E-24/c15, E-15/c07).

## Acceptance Criteria

**AC-1: Address errors beside the field (AC-54, IT-15-1)**
Given a stub answering `set_server` in turn with `HttpsRequired`, `ServerUnreachable`, `UpgradeRequired{message: "Please update to 1.2"}` and `SignedIn`
When the user applies an address four times
Then each message appears beside the address field, the third shows "Please update to 1.2", and no raw error code is shown

**AC-2: Applied address**
Given a stub that accepts `set_server`
When the user applies `https://cloud.example`
Then exactly one `set_server` call is sent with that string, the "Applied" mark appears, and the field shows the `server_url` of the next status

**AC-3: Sign-in prompt (IT-15-8)**
Given a stub answering `login_start` with `LoginPrompt{verification_url, user_code: "WDJB-MJHT", expires_at}` and `login_status` with Pending three times, then Approved
When the user starts sign-in
Then the URL and `WDJB-MJHT` are visible and can be copied, the expiry time is shown, the status text follows Pending, and after Approved the screen shows the account and the sign-out button

**AC-4: Denied and Expired (IT-15-8)**
Given separate runs where `login_status` ends in Denied and in Expired
When each run reaches its final state
Then a message names the outcome and a retry starts a new `login_start`

**AC-5: Server not set (IT-15-1)**
Given a stub answering `login_start` with `ServerNotSet`
When the user presses sign-in
Then the message appears beside the sign-in button

**AC-6: Never a password or token (AC-48, IT-15-8)**
Given every state of the Account screen
When the accessibility tree of the screen is walked
Then it holds no password field and no field that asks for a token

**AC-7: Sign-out warning**
Given a stub answering `logout` with `server_revoked = false`, and in a second run with `server_revoked = true`
When the user signs out
Then the first run shows the warning to remove the device on the web, the second shows none, and both return the screen to the signed-out state

## Non-Functional Requirements

**Security**
- The user code appears on screen only; it is never written to the log at INFO (ST-15-3, checked by E-25/c13).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Error-to-field mapping for the four `set_server` errors and `ServerNotSet` | Each maps to its control |
| AC-3 | Login state to screen state | Pending, Approved, Denied, Expired, SignedOut each map to one view |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Stub error script | Apply four times | Messages beside the field | IT-15-1, NFT-SEC-12 |
| AC-2 | Stub accepts | Apply address | One call; Applied mark | IT-15-1 |
| AC-3 | Stub login script | Sign in | URL and code; Pending; Approved view | IT-15-8, AC-48 |
| AC-4 | Denied run; Expired run | Sign in | Message and retry | IT-15-8 |
| AC-5 | Stub `ServerNotSet` | Sign in | Message beside the button | IT-15-1 |
| AC-6 | All states | Tree walk | 0 password or token fields | ST-15-2 |
| AC-7 | Stub logout outcomes | Sign out | Warning only when not revoked | IT-15-8 |

## Constraints

- One `DaemonControl` call per user action; the window holds no hidden state (15 §5).

## Risks & Mitigation

**Risk 1: The code is unreadable for a user reading it aloud**
- *Risk*: a small or proportional font causes typing errors on the approval page.
- *Mitigation*: the code is shown large in a mono font, as the mockup draws it.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `AccountView` holds only `account_id` (`ipc_schema.md` open point 3), but the mockup shows the user's name and the device name ("Signed in as Marta Hryn", "zx-thinkpad") | E-24 / owner | open | Medium |
| The device name passed to `login_start` has no stated source (user input or the computer name) | E-24 / E-25 owners | open | Low |
| How often the window reads `login_status` (or whether it waits for status events) is not stated | E-25 | open | Low |
| The mockup's "Open browser" button has no spec: the daemon opens the browser at `login_start`; re-opening from the window is not specified, so it is not built | E-25 / owner | open | Low |
| `LoginPrompt.expires_at` has no source in 13's device authorization (`ipc_schema.md` open point 5) | E-23 / E-24 | open | Low |
