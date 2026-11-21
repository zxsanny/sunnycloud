# Svelte conventions for 16_web_app

Written before the scaffold, as `~/.claude/rules-optional/ui-framework.md` asks. It applies to `src/web/**`. Where it
disagrees with the component description, the description wins; flag the difference.

## Language and framework

- Svelte 5 with **runes only**: `$state`, `$derived`, `$effect`, `$props`, `$bindable`, `$props.id()`. Never `export let`,
  `$:`, stores (`writable`), `<svelte:component>`, `on:click` or `createEventDispatcher`. `svelte.config.js` compiles our own
  components in runes mode, so a legacy construct is a build error. Events are plain props (`onclick`, `onpick`);
  content is `Snippet` props.
- TypeScript strict. No `any`; narrow `unknown`.
- Shared reactive state lives in a `.svelte.ts` module as one class instance (`session`, `notices`, `theme`, `route`),
  never as a reassigned exported `$state` variable.
- A page loads its data in `onMount` through the API facade and renders three states: loading (skeleton), failed (banner
  with a retry), ready.

## Layout

- `src/lib/<module>/` is a module with a **barrel** `index.ts`. Everything outside the module imports `$lib/<module>`
  only; `$lib/<module>/<file>` is a lint error. Inside a module, use relative imports.
- Modules: `api` (generated client, wrapper, `ApiError`), `router`, `ui` (shared components), `theme`, `notices`,
  `session`, `auth`, `format`, `files` (folder store, preview and render rules), `transfer` (the upload facade, the Worker
  and its engine).
- `src/pages/` pages and the signed-in shell; `src/app/` the root component, the route table and the app-level error
  mapping. Pages call the server only through `$lib/api`.
- One component per file, PascalCase; logic that is not about markup goes in a `.ts` module next to it. At most 500 lines
  per file.
- Tests sit beside the code as `<name>.test.ts`. Test-only components live in a `fixtures/` folder. Cross-cutting tests
  and helpers live in `tests/`.

## Worker and WASM

- Hashing and every upload request run in the module Worker of `lib/transfer`, never on the main thread. The engine
  (`engine.ts`) takes its WASM, API client and `post` function as arguments, so Node tests and the manual large-file
  check run the same code without a Worker.
- `sync-core-wasm` is built by `scripts/build-wasm.sh` into `wasm/pkg` (gitignored) and imported through `$wasm/...`.
  The `.wasm` stays a separate content-hashed file (`?url`, `assetsInlineLimit: 0`); a bundle test fails if it is inlined.
- Block hashes, roll-ups, commit IDs and the upload planner come only from that module; do not write a JavaScript copy.

## Security (the CSP is `script-src 'self' 'wasm-unsafe-eval'`, `style-src 'self'`)

- No `{@html}`, `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `createElement` for data, `document.write`. Untrusted
  names and media types are rendered as text. Dynamic data goes through components and `{#each}`/`{#if}`.
- No inline `<script>`, no inline event-handler attributes, no `style="..."` attribute in markup, no `<style>` in
  `index.html`. Svelte `onclick={...}` is fine: it is not an HTML attribute in the output.
- No `fetch`, `XMLHttpRequest`, `EventSource` outside `src/lib/api`. No `document.cookie`. No token in
  `localStorage`, `sessionStorage` or IndexedDB (the session is an HttpOnly cookie).
- The only navigation to another origin is `followExternal(authorizationUrl)` after `isFollowableUrl`. Return paths pass
  `safeReturnPath`.
- A `catch` names one expected error type or reports to the console or the user; never empty.

## Styling

- Colours come only from `src/styles/tokens.css`, a verbatim copy of `_docs/02_document/ui_mockups/assets/tokens.css`
  (a test fails when they differ). Edit the source, then copy.
- Tailwind utilities do layout, spacing, type and colour. `@theme inline` in `src/styles/app.css` maps them to the
  tokens, and clears Tailwind's own palette, radii, fonts, shadows and sizes, so only token-backed classes exist:
  `bg-surface`, `text-ink-2`, `rounded-control`, `text-h1`, `font-code`, `shadow-card`.
- No new `.css` file and no `<style>` block per page or component. Shared look lives in the `$lib/ui` components.
- Themes: `color-scheme` plus `light-dark()` in the tokens; `data-theme` on `<html>` is the stored override.
- Every interactive element keeps the global `:focus-visible` ring. A disabled control uses the disabled tokens (never
  opacity) and says why in `title` (`Button`'s `disabledReason`).

## Tests

- Vitest with jsdom and Testing Library. Arrange, Act, Assert with comments, one behaviour per test, a name that states
  the behaviour. No quiet flags: `bun run test` prints every test.
- Unit tests stub the network with `stubServer` (`tests/support/http.ts`), which answers by `"METHOD /path"` and fails the
  test on a request nobody planned. That proves the client and the pages against the documented answers, not against
  the server.
- Query by role and visible text (`getByRole`, `findByText`), as a user would; no test ids.
- The production bundle is built for real in `tests/bundle.test.ts` and scanned (inline script or handler, foreign URL).
