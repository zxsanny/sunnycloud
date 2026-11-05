# Design System: sunnycloud

Owner direction (2026-11-04): two themes, light "a bit gray" and dark "graphite"; accents yellow and blue; visually appealing. Applies to every client: web (16_web_app, Svelte 5), desktop settings window (15_desktop_settings, Slint), Android (17_android_app, Flutter) and iOS (18_ios_app, v1.1, same Flutter UI).

Mockups in this folder are the visual reference for task specs. The tokens in `assets/tokens.css` are the single source of colour values; this file explains them.

## 1. Visual Atmosphere

**Sun over a cloud.** A calm, slightly overcast gray canvas (light) or a soft graphite workspace (dark), with one clear sky-blue for everything you can act on and a small, warm dose of sun-yellow that marks the brand, the current place and good news. Surfaces are quiet and layered by tone rather than by heavy shadows; content (folders, photos, transfer progress) carries the colour. Density is comfortable, not airy: a file list shows 12 to 14 rows on a laptop without feeling cramped. The mood is trustworthy and personal, closer to a well-kept home server than to a corporate drive.

Two accents, two jobs (this resolves the "one strong accent per view" rule):

| Accent | Job | Never used for |
|---|---|---|
| **Sky blue** (`--action`) | primary buttons, links, focus ring, progress, selected item | decoration |
| **Sun yellow** (`--sun`) | brand mark, active-nav indicator, highlight chips ("Admin", "New", one-time link panel), completed-backup sun | warnings, errors, body text on light backgrounds |

Warnings are **orange**, errors **red**, success **green**: yellow never carries a status, so "highlight" and "careful" cannot be confused.

## 2. Color System

All tokens are CSS custom properties. Themes switch with the CSS `light-dark()` function: each semantic token is defined once with its light and dark value, and `color-scheme` picks one. The default follows the OS (`color-scheme: light dark`); `data-theme="light|dark"` on `<html>` forces one (web stores that choice in `localStorage` as a UI preference, wrapped in try/catch). Gotcha: inherited properties such as `color` resolve `light-dark()` on the element that declares them, so any nested region with its own `color-scheme` must declare `color` again (only `index.html` does this; the product switches at the root).

### Primitives

```css
/* Graphite: neutral with a slight cool cast */
--g-0:   #FFFFFF;  --g-25:  #F5F6F8;  --g-50:  #EEF0F3;  --g-75:  #E9ECF0;
--g-100: #DFE3E8;  --g-150: #D3D8DF;  --g-200: #BCC2CB;  --g-250: #A4ABB5;
--g-300: #868E9B;  --g-400: #757C87;  --g-500: #5F6774;  --g-600: #4F5763;
--g-700: #424850;  --g-750: #30353B;  --g-800: #262A2F;  --g-850: #1E2125;
--g-900: #16181B;  --g-950: #101114;

/* Sky blue: action */
--b-50:  #EEF4FE;  --b-100: #DCE7FD;  --b-200: #B9D1FC;  --b-300: #82AEFA;
--b-400: #5B95F8;  --b-500: #2F6BE0;  --b-600: #2257CC;  --b-700: #1C47A8;
--b-800: #1B3F91;  --b-900: #172F66;  --b-950: #1C2A44;  --b-ink: #0B1426;

/* Sun yellow: brand and highlight */
--s-50:  #FFF8E1;  --s-100: #FFEFB8;  --s-200: #FFE38A;  --s-300: #FFD95A;
--s-400: #FFC72C;  --s-500: #F2B705;  --s-700: #A27600;  --s-800: #6E4F00;
--s-900: #3A2C00;  --s-950: #2A1F00;

/* Status */
--green-700: #17703F;  --green-300: #5BD39A;  --green-50: #E3F4EA;  --green-950: #17301F;
--orange-800: #8F3409; --orange-700: #B4410B; --orange-300: #FF9F66; --orange-200: #FFB98F;
--orange-50: #FDEBDD;  --orange-950: #3A2216;
--red-800: #912018;    --red-700: #B42318;    --red-300: #FF7A70;    --red-200: #FF9A92;
--red-50: #FDE7E5;     --red-950: #3D1C1C;
```

### Semantic tokens (light | dark)

| Token | Light | Dark | Use |
|---|---|---|---|
| `--bg-page` | g-75 `#E9ECF0` | g-900 `#16181B` | viewport, sidebar |
| `--bg-surface` | g-25 `#F5F6F8` | g-850 `#1E2125` | main panel, cards, tables |
| `--bg-raised` | g-0 `#FFFFFF` | g-800 `#262A2F` | modals, menus, inputs, transfer panel |
| `--bg-sunken` | g-100 | g-750 | progress track, skeletons, code blocks |
| `--bg-hover` | ink 5% | white 6% | row and ghost-button hover |
| `--bg-overlay` | ink 45% | black 60% | modal scrim |
| `--text-primary` | g-900 | g-75 | body, titles |
| `--text-secondary` | g-600 | g-250 | meta, helper text |
| `--text-tertiary` | g-500 | g-300 | placeholders, disabled labels (still 4.5:1) |
| `--border` | g-150 | g-750 | dividers, card edges |
| `--border-strong` | g-300 | g-400 | input borders (3:1 against fill) |
| `--action` | b-600 | b-400 | primary button, progress |
| `--action-hover` / `--action-active` | b-700 / b-800 | b-300 / b-200 | button states |
| `--on-action` | white | b-ink | text on `--action` |
| `--action-subtle` / `--on-action-subtle` | b-100 / b-800 | b-950 / b-200 | selected row, active nav, info banner |
| `--link` | b-700 | b-300 | inline links |
| `--focus-ring` | b-500 | b-400 | 2px ring, 2px offset |
| `--sun` | s-500 | s-400 | brand mark, nav indicator bar (decorative) |
| `--sun-strong` | s-700 | s-400 | sun icons that must meet 3:1 |
| `--sun-fill` / `--on-sun` | s-300 / s-900 | s-400 / s-950 | highlight chips and panels |
| `--success` / `--success-bg` | green-700 / green-50 | green-300 / green-950 | |
| `--warning` / `--warning-bg` / `--on-warning-bg` | orange-700 / orange-50 / orange-800 | orange-300 / orange-950 / orange-200 | holds, mismatch, paused |
| `--danger` / `--danger-bg` / `--on-danger-bg` | red-700 / red-50 / red-800 | red-300 / red-950 / red-200 | errors, destructive buttons |

Measured contrast (WCAG 2.2): body text 13.4 to 15.6:1; secondary 6.2 to 7.0:1; tertiary 4.8 to 4.9:1; button text 6.2 to 6.4:1; links 6.4 to 7.2:1; status text 5.3 to 8.6:1; input borders and focus ring 3.1 to 6.0:1. Yellow is never used as text on a light background (1.5:1).

### Component tokens

```css
--btn-primary-bg: var(--action);        --btn-primary-text: var(--on-action);
--btn-secondary-bg: var(--bg-raised);   --btn-secondary-border: var(--border-strong);
--btn-danger-bg: var(--danger);         --btn-danger-text: var(--on-action);  /* dark: b-ink on red-300 */
--card-bg: var(--bg-surface);           --card-border: var(--border);
--input-bg: var(--bg-raised);           --input-border: var(--border-strong);
--nav-active-bg: var(--action-subtle);  --nav-active-text: var(--on-action-subtle);
--nav-indicator: var(--sun);            /* 3px bar left of the active item */
--progress-track: var(--bg-sunken);     --progress-fill: var(--action);
--shadow-color: light-dark(rgb(16 17 20 / 0.08), rgb(0 0 0 / 0.45));
```

## 3. Typography

- **UI font**: **Manrope** (variable, 400 to 800), fallback `system-ui, sans-serif`. Geometric but warm; distinct from the default Inter/Roboto look; tabular figures (`font-variant-numeric: tabular-nums`) for sizes, counts and progress.
- **Mono font**: **JetBrains Mono** (400, 600) for device user codes, setup tokens, file hashes, correlation ids, share URLs and rule patterns.
- **Source**: both are SIL OFL fonts. Production bundles them: web serves self-hosted `woff2` from its own origin (the CSP allows no third-party hosts), Slint embeds the `.ttf` with `import`, Flutter declares them as font assets. Mockups load them from Google Fonts for preview only.

| Level | Element | Size | Weight | Line height | Tracking |
|---|---|---|---|---|---|
| Display | sign-in and public-share headline | 2.25rem (36px) | 800 | 1.15 | -0.02em |
| H1 | page title | 1.625rem (26px) | 750 | 1.25 | -0.01em |
| H2 | section title | 1.25rem (20px) | 700 | 1.3 | 0 |
| H3 | card, dialog section | 1.0625rem (17px) | 700 | 1.4 | 0 |
| Body | default | 1rem (16px) | 450 | 1.5 | 0 |
| Row | table and list rows | 0.9375rem (15px) | 500 | 1.4 | 0 |
| Small | meta, helper text | 0.875rem (14px) | 450 | 1.45 | 0.005em |
| Label | overline, column heads, badges | 0.75rem (12px) | 700 | 1.3 | 0.06em, uppercase |
| Code | mono values | 0.875rem (14px) | 500 | 1.4 | 0.02em |

Desktop window (Slint) uses one step smaller: body 14px, row 14px, H1 20px, matching native settings windows.

## 4. Spacing & Layout

- **Base unit** 4px. Scale: 4, 8, 12, 16, 20, 24, 32, 40, 48, 64.
- **Radius**: `--radius-sm` 6px (inputs, buttons, chips), `--radius-md` 10px (cards, panels, thumbnails), `--radius-lg` 14px (modals, phone sheets), `--radius-full` (avatars, pills, toggles).
- **Elevation**: tone first, shadow second. Light: raised surfaces get `0 1px 2px var(--shadow-color)`; menus and modals `0 12px 32px var(--shadow-color)`. Dark: raised surfaces are lighter (g-800) plus a 1px `--border`; shadows only on modals. Never shadow + border + fill change on one element at once.

**Web shell** (holy grail): sidebar 248px on `--bg-page` | main panel on `--bg-surface` with `--radius-md` top-left corner and 1px border, top bar 64px inside the panel. Content max-width 1440px.

| Breakpoint | Min width | Layout | Gutter |
|---|---|---|---|
| sm | 0 | single column; sidebar becomes a drawer behind a menu button; table rows become two-line list items; transfer panel becomes a bottom sheet | 16px |
| md | 768px | sidebar collapses to a 72px icon rail | 24px |
| lg | 1024px | full 248px sidebar; file table with all columns | 32px |
| xl | 1280px | wider name column; detail pane may open beside the list | 32px |

Touch targets at least 44px on sm; 36 to 40px controls on lg and up.

## 5. Component Styling Defaults

**Buttons** (sm 32px, md 40px, lg 48px; padding 0 16px; weight 650; radius sm):
- Primary: `--action` fill, `--on-action` text. One per visible area.
- Secondary: `--bg-raised` fill, 1px `--border-strong`, `--text-primary` text.
- Ghost: no fill; `--bg-hover` on hover. Used for row actions and toolbars.
- Danger: `--danger` fill for confirmed destructive actions; danger-ghost (red text, no fill) for "Revoke" and "Remove" in lists.
- Hover darkens one ramp step (light) or lightens one step (dark); active one more step; transition `background-color 150ms ease`.

**Inputs** (40px; radius sm; `--input-bg`; 1px `--input-border`; label above, 14px weight 650; helper text below in `--text-secondary`): focus = border `--action` + 2px `--focus-ring` ring; error = border `--danger`, message below with alert icon; mono inputs for codes and tokens.

**Chips and badges** (24px, radius full, Label type): neutral (`--bg-sunken`), highlight (`--sun-fill` / `--on-sun`: "Admin", "This browser", "New"), status (success/warning/danger bg with matching text and an icon, never colour alone).

**File list** (table on lg, list on sm): sticky header in Label type, row 52px, 40px rounded thumbnail at the start, name in Row type, meta in Small type, hover `--bg-hover`, selected `--action-subtle` with a checkbox. Folders first. Untrusted names are text only and truncate with an ellipsis plus the full name in `title`.

**Thumbnails**: `ready` shows the image (radius md, `object-fit: cover`); `pending` shows a skeleton pulse; `failed` and `none` show the type icon on `--bg-sunken`. Video tiles add a duration chip; non-inline types never get an `<img>`.

**Progress**: 6px bar, radius full, `--progress-track` / `--progress-fill`, numeric percent and bytes in tabular figures next to it. Paused bars switch to `--warning` with a pause icon and text.

**Navigation**: sidebar items 40px, icon + label; active = `--nav-active-bg` fill, `--nav-active-text`, weight 700 and a 3px `--nav-indicator` bar; hover `--bg-hover`. Admin items sit in their own labelled group, shown only for the admin role.

**Modals**: max 560px (share dialog) or 640px; header with title and close button, body, footer actions right-aligned (primary right). Scrim `--bg-overlay`. Focus trapped; Escape closes. No modal on a modal: confirmation happens inline.

**Banners**: full width of their container, icon left, text, optional action right; info (`--action-subtle`), success, warning, danger.

**Brand mark**: a sun (`--sun`) rising behind a cloud (`--action`); wordmark "sunnycloud" in Manrope 800, lower case.

**Icons**: one outline set, 24px grid, 1.75px stroke, round caps (Lucide style; production web uses `lucide-svelte`, Flutter `lucide_icons`, Slint embeds the same SVGs). Provider sign-in buttons use each provider's official mark per its brand guidelines; the mockups show a neutral letter mark in its place.

## 6. Interaction States

- **Loading**: skeletons in the shape of the content (rows with a 40px thumb block and two text bars), pulse `opacity 0.45 to 1`, 1.4s ease-in-out on `--bg-sunken`. Spinners only inside buttons for waits under 3 s. `prefers-reduced-motion` stops the pulse.
- **Empty**: 64px illustration (sun and cloud line art), one-sentence explanation, one primary action ("Upload files", "Create invite", "Add folder pair").
- **Error**: inline message under the field with an alert icon; page-level errors as a danger banner with the correlation id in mono and a copy button. Mapped API errors: 401 to sign-in; 426 "A newer version is required" with reload; 429 "Try again in N seconds" with a live countdown; 507 "Storage quota reached" with the quota numbers.
- **Hover**: background shift (`--bg-hover`) or one-step darker fill; links underline.
- **Focus**: 2px `--focus-ring` with 2px offset on every interactive element (`:focus-visible`); never removed.
- **Disabled**: `--text-tertiary` text, `--bg-sunken` fill, `cursor: not-allowed`, and a reason next to it or in `title` (e.g. "Folder sharing is not available in this version"). No opacity: Slint's software renderer has no layer opacity, so all clients use the explicit tokens.
- **Applied** (desktop settings): after a `set_settings` echo, a small check and "Applied" appear next to the control for 2 s; on failure the control reverts and shows the error inline.

## 7. Platform Mapping

| Client | Token home | Theme switch | Constraints |
|---|---|---|---|
| Web (Svelte 5) | `src/lib/styles/tokens.css` (copy of `assets/tokens.css`), Tailwind theme maps colours to the CSS variables, component styles use the tokens | `color-scheme` + `light-dark()`; `data-theme` override | self-hosted fonts; no inline styles or scripts (CSP) |
| Desktop settings (Slint 1.18) | `ui/theme.slint` global `Theme` with the same token names | `Palette.color-scheme` follows the OS | software renderer: no transforms, no layer opacity, avoid blurred shadows (flat surfaces with borders) |
| Android / iOS (Flutter) | `ThemeData` light and dark from one `SunnyTokens` class; `ColorScheme.primary` = action blue, `tertiary` = sun | `ThemeMode.system` | Material 3 widgets restyled with the tokens; iOS keeps the same tokens (v1.1) |

## 8. Screens

| File | Screens and states | Component / ACs |
|---|---|---|
| `index.html` | gallery, palette in both themes | all |
| `web-files.html` | file browser, transfers panel, thumbnail states, resume banner, download verified/unverified, empty, loading, API errors | 16; AC-3, AC-4, AC-7, AC-50, AC-51, AC-64 |
| `web-share-dialog.html` | create link, link shown once, list and revoke, folder share disabled | 16, 06; AC-12, AC-77 |
| `web-public-share.html` | inline preview, download-only type, password, link not available, rate limited | 16; AC-12, AC-47 |
| `web-device-approval.html` | enter code, location match, mismatch warning, approved, denied, expired | 16, 01; AC-57 |
| `web-sign-in.html` | providers (AC-19: GitHub + OIDC/Google v1.0, Apple v1.1), invite, not invited, deactivated, first-run setup token and step 2 | 16, 01; AC-19, AC-23 |
| `web-account.html` | profile, storage, devices and sessions with revoke | 16, 01 |
| `web-admin.html` | users, invites, quotas, health (`?tab=` opens a tab); last-admin refusal, invite created, no invites, health degraded | 16, 07, 07a |
| `desktop-settings.html` | account and device sign-in, folders, rules, settings, status, attention (mass-delete hold), daemon not running, tray menu | 15, 14; AC-16, AC-17, AC-29, AC-66 |
| `android-backup.html` | sign-in, backup status, limited access, paused (quota), settings with battery guidance | 17 (18 reuses); AC-9, AC-53 |

Not drawn: vault page (v1.2 placeholder only), iOS-specific screens (v1.1, same Flutter UI with the Sign in with Apple rule).

## 9. Spec Findings From the Mockups (2026-11-04)

Drawing the screens against the component specs turned up these points. The mockups follow the specs; the rows marked **decision** need the owner (or a spec fix) before the matching tasks are decomposed.

| # | Finding | Where | Mockup choice |
|---|---|---|---|
| 1 | **Decision.** The web app lists "delete user" for admins, but the admin API (07) has only `PATCH {role, state}` for another user. | 16 §5 vs 07 | "Delete account…" drawn in the user menu; add the endpoint or drop the item |
| 2 | README and `problem.md` still name Facebook sign-in; `restrictions.md` puts it out of scope and AC-19 is GitHub + generic OIDC (Google preset) in v1.0, Apple in v1.1. | README, 00_problem | Sign-in shows Google, Apple, GitHub; fix the README at Update Docs |
| 3 | The Devices list merges desktop device tokens (`/devices`) and phone sessions (`/sessions`, `client_kind` android/ios); neither carries an app version. | 01 data model | Rows show name, platform, added, last seen |
| 4 | The health endpoint returns a list of checks (`ok/degraded/down` + detail), not fixed "storage free / queue depth / workers" fields. | 11, 07 | Health tab: status banner, three figures, checks and workers tables |
| 5 | Default quota is server configuration (`default_quota_bytes`); per-user quota can be set or cleared (null = default). | 07a | Default shown read-only |
| 6 | Invite expiry comes from `invite_ttl`; create takes only a note. The invite URL format is not specified. | 01, 07 | "Expires after 7 days (server setting)"; placeholder `/invite#sc_inv_…` |
| 7 | **Decision.** Desktop bandwidth limit has no unit in the spec (tests use "5000"). | 15 §5 | Drawn as KB/s |
| 8 | Share dialog shows the server's maximum link lifetime; 06's default `share_max_ttl` is 365 days. | 06 | Example "30 days"; the UI reads the real value from the API |
| 9 | When the setup token is checked is not specified. | 01, 12 | Neutral error wording |
| 10 | Not drawn: macOS access-denied (v1.1), Linux no-dialog-backend message, typed `set_server`/`add_pair` errors, approval with unknown location or `AlreadyDecided`, Android user-added-CA warning. | 15, 01, 17 | Left for the task specs |
