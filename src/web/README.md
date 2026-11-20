# sunnycloud web app

Svelte 5 single-page application (component 16, `_docs/02_document/components/16_web_app/`). It owns no server logic and holds no
tokens: the session is an HttpOnly cookie that the server sets. The build is static files (`dist/`) that Caddy serves
next to the `/api` proxy. Conventions: `_docs/02_document/components/16_web_app/svelte_conventions.md`.

## Commands

Node 22 or newer and npm (`engines` and `.npmrc` enforce it).

| Command                                             | What it does                                                                                         |
| --------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `npm ci`                                            | Installs the exact versions of `package-lock.json`.                                                  |
| `npm run dev`                                       | Vite dev server on http://localhost:5173. `/api` is proxied to `http://127.0.0.1:8080` (the server). |
| `npm run build`                                     | Production build into `dist/` (gitignored).                                                          |
| `npm run lint`                                      | ESLint and `prettier --check`.                                                                       |
| `npm run check`                                     | `svelte-check` (types for `.svelte` and `.ts`).                                                      |
| `npm test`                                          | Vitest, verbose. Includes a real production build and its CSP scan.                                  |
| `npm run gen:api`                                   | Regenerates the typed client sources from `openapi/openapi.json`.                                    |
| `npm run gen:api -- --refresh <path/to/sunnycloud>` | First replaces the snapshot with the output of `<binary> openapi`.                                   |
| `src/web/ci/web-ci.sh`                              | All of the above in CI order, plus client drift and `npm audit`. The `web` job in `ci.yml` runs it.  |

The dev proxy does not rewrite `Origin` or `Host`: the server's CSRF check accepts a cookie request whose
`Sec-Fetch-Site` is `same-origin`, and the browser sends that for the dev server's own origin.

## The API client

`openapi/openapi.json` is the server's document, byte for byte (`sunnycloud openapi`). `scripts/gen-api.mjs` turns it into
`src/lib/api/generated/schema.d.ts` (types) and `problem-codes.ts` (the known error codes). Both are committed; CI
regenerates them and fails on any difference. After a server API change: build the server, run
`npm run gen:api -- --refresh <binary>`, commit the three files.

`src/lib/api` is the facade. `createApi()` returns one method per operation the app uses; each returns
`Result<T, ApiError>` and never throws for a server or network failure. It adds `X-Request-Id` (UUIDv7) and
`X-Sunnycloud-Client: web/<package version>`, uses same-origin credentials, adds no `Authorization` or CSRF header,
maps an unknown code or a non-JSON error body to `internal`, and stops sending after a 426. A method is added when a
page needs it. `src/app/handleApiError.ts` is the one place where 401, 426, 429, 507, 5xx and network failures become
notices (`isAppLevelError` says which failures are the app's; the rest go to the calling page).

## Dependencies

Every dependency is pinned exactly in `package.json`; `package-lock.json` is committed; the registry is
registry.npmjs.org.

| Package                                                                                                        | Job                                                                                             |
| -------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `svelte`                                                                                                       | The framework (runes). 5.57.3 is the version the packaging spike AZ-3930 measured.              |
| `vite`, `@sveltejs/vite-plugin-svelte`                                                                         | Build, dev server, Svelte compiler integration (spike versions).                                |
| `tailwindcss`, `@tailwindcss/vite`                                                                             | Utility classes mapped to the design tokens (DESIGN.md section 7).                              |
| `openapi-fetch`                                                                                                | The small typed `fetch` client (about 6 kB) that the wrapper is built on.                       |
| `openapi-typescript` (dev)                                                                                     | Generates the types from the OpenAPI document.                                                  |
| `@lucide/svelte`                                                                                               | The icon set of DESIGN.md (Lucide); imported per icon, so only used icons ship.                 |
| `@fontsource-variable/manrope`, `@fontsource/jetbrains-mono`                                                   | The two fonts as woff2 files served from our own origin (the CSP allows no font host). SIL OFL. |
| `typescript` 5.9.3 (dev)                                                                                       | Types. Not 7: `svelte-check` and `openapi-typescript` accept only 5.x and 6.x as peers.         |
| `svelte-check` (dev)                                                                                           | Type check of `.svelte` files.                                                                  |
| `eslint`, `@eslint/js`, `typescript-eslint`, `eslint-plugin-svelte`, `globals` (dev)                           | Lint, including the rules that keep the bundle inside the CSP.                                  |
| `prettier`, `prettier-plugin-svelte` (dev)                                                                     | Formatting.                                                                                     |
| `vitest`, `jsdom`, `@testing-library/svelte`, `@testing-library/user-event`, `@testing-library/jest-dom` (dev) | Unit and component tests.                                                                       |
| `@types/node` (dev)                                                                                            | Types for the tests and config that use `node:` modules.                                        |

## The router (`src/lib/router`)

About 100 lines of our own instead of a router package: the app has a handful of fixed paths, no nested layouts and no
data loading by route. It holds the location in a reactive object, matches exact paths from a table (`src/app/routes.ts`),
renders the matching page or the not-found page, follows same-origin link clicks without a page load (links under
`/api/`, new-tab links and downloads stay with the browser), and handles back and forward. Dynamic segments
(`/s/:token`, `/files/:id`) are added with the first page that needs one.

## Routes today

`/` (signed-in start page, a stub), `/login`, `/invite#<token>` (the sign-in page in its invite state; the format follows the
placeholder in the mockup and is an open point of AZ-3936), `/setup`. Not built yet: file browser, uploads, shares,
device approval, admin, account page, vault.
