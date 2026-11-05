# Design tokens, self-hosted fonts, shared components and app shell

**Task**: AZ-3956_design_tokens_components
**Name**: Design system for the web app
**Description**: Bring the owner-approved design system into the web app: the token stylesheet with light and dark themes, self-hosted Manrope and JetBrains Mono under the strict CSP, the shared components and interaction states, and the responsive app shell
**Complexity**: 5 points
**Dependencies**: AZ-3935_spa_scaffold
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3956
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/n02

## Dependencies

### Document Dependencies
- `_docs/02_document/ui_mockups/DESIGN.md` — tokens, typography, spacing, components, interaction states, platform mapping (approved 2026-11-04).
- `_docs/02_document/ui_mockups/assets/tokens.css` — the single source of colour values.
- `_docs/02_document/contracts/16_web_app/api_client.md` — session profile and quota view for the shell, produced by E-26/c06.

## Problem

The owner approved the mockups and the design system on 2026-11-04. Without one shared set of tokens and components, every page task would restyle buttons, banners and dialogs on its own, the themes would drift, and inline styles or third-party fonts would break the strict CSP (`style-src 'self'`, no third-party host).

## Outcome

- One token stylesheet, a copy of `assets/tokens.css`, is the only source of colours; light and dark follow the OS, with a stored override.
- Fonts load from the app's own origin; the bundle has no inline style or script.
- Shared components and states from DESIGN §5 and §6 exist once and every page uses them.
- The app shell (sidebar, rail, drawer, top bar) works at every breakpoint of DESIGN §4.

## Scope

### Included
- Token stylesheet and its drift check against `assets/tokens.css`; the colour mapping of the styling framework to the tokens (DESIGN §7).
- Themes: `color-scheme` plus `light-dark()`; `data-theme` override kept as a UI preference in `localStorage`, wrapped so blocked storage falls back to the OS.
- Fonts: Manrope (variable) and JetBrains Mono as self-hosted `woff2`; type scale of DESIGN §3; tabular figures for numbers.
- Components: buttons (primary, secondary, ghost, danger, danger-ghost; sm, md, lg; in-button spinner), inputs (label, helper, error, mono), chips and badges (neutral, highlight, status with icon), banners (info, success, warning, danger; optional action; request id with copy), modal (560 or 640 px, focus trap, Escape, no modal on a modal), progress bar (paused state), skeleton, empty state, icons from the one outline set, brand mark.
- States: loading, empty, error, hover, focus ring on every interactive element, disabled with a visible reason and no opacity, reduced motion.
- App shell: 248 px sidebar with the active indicator, the admin group shown only for the admin role, the storage meter from the quota view, the account menu; 72 px icon rail at md; drawer at sm; a slot that becomes a bottom sheet on sm for the transfers panel; content up to 1440 px.

### Excluded
- Page content and page-specific layouts (E-26/c07a to c18, n01); the file list row layout lives with the file browser (E-26/c08).

## Acceptance Criteria

**AC-1: One source of colours**
Given the built app and `assets/tokens.css`
When the drift check runs, and a value is changed in one of them only
Then the clean state passes with equal values, and the one-sided change fails the check

**AC-2: Themes and override**
Given the OS in light mode, then dark mode, and the user choosing dark in the app
When the app loads and reloads
Then the tokens follow the OS, the user's choice survives a reload, and with storage blocked the app follows the OS with no error

**AC-3: Fonts from the own origin**
Given the production build behind the 16 §5 CSP
When any page loads
Then Manrope and JetBrains Mono load from the app's origin, 0 font requests go to another host, 0 CSP violations are reported, and a blocked font falls back to `system-ui`

**AC-4: No inline styles or scripts**
Given every shared component rendered in both themes
When the build output and the rendered DOM are scanned under `style-src 'self'`
Then there is no inline `<style>` element or `style` attribute from markup, no inline script, and 0 CSP violations

**AC-5: Contrast in both themes**
Given the text, border and focus pairs of DESIGN §2
When the automated contrast check runs in light and dark
Then body, secondary and tertiary text reach at least 4.5:1, input borders and the focus ring at least 3:1, and no yellow is used as text on a light background

**AC-6: Interaction states**
Given a modal, a disabled button, a skeleton and a form field in error
When they are used by keyboard and with reduced motion
Then the modal traps focus and closes on Escape, every interactive element shows the 2 px focus ring on keyboard focus, the disabled button shows its reason and uses the disabled tokens (not opacity), the field shows its message with an icon, and the skeleton pulse stops under reduced motion

**AC-7: Responsive shell and role**
Given widths of 375, 768, 1024 and 1280 px, an admin and a plain user
When the shell renders
Then sm shows the drawer with touch targets of at least 44 px, md the 72 px rail, lg and xl the 248 px sidebar; the admin group shows only for the admin; the storage meter equals the quota view

**AC-8: App notices use the banner**
Given the app-level notices of E-26/c06 (upgrade, rate limit, quota, server error)
When each is raised
Then it renders as the matching banner variant with its action, and the server-error banner shows the request id in mono with a working copy action

## Non-Functional Requirements

**Compatibility**
- Chromium, Firefox and Safari current releases; DESIGN §4 breakpoints.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Token drift check with a seeded difference | Fails on the difference |
| AC-5 | Contrast calculator over the token pairs in both themes | Every pair at or above its threshold |
| AC-6 | Modal focus trap and Escape | Focus stays inside; Escape closes |

## Blackbox Tests

Playwright against the real stack (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | OS light and dark; storage blocked | Theme selection | Follows OS; override kept | ST-16-2 |
| AC-3, AC-4 | 16 §5 CSP | Fonts and inline scan | Own origin; 0 violations | IT-16-9 |
| AC-6, AC-7 | Keyboard; four widths; two roles | States and shell | As stated | |
| AC-8 | Fault proxy statuses | Notices | Banner variants; copy works | IT-16-14 |

## Constraints

- Tokens come only from `assets/tokens.css`; DESIGN §7 names the token home and the styling-framework mapping, and the icon set for the web.
- Dependency versions match those already pinned in the repository and are checked to exist at the pinned version.

## Risks & Mitigation

**Risk 1: Framework-generated inline styles**
- *Risk*: a component library writes `style` attributes that the CSP blocks.
- *Mitigation*: AC-4 scan of the rendered DOM under the real CSP.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The font files are SIL OFL and the icon set's licence must be compatible with AGPL distribution (ADR-011); record both in the licence notes | E-26 owner | open | Low |
