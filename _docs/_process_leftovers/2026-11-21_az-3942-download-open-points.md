# AZ-3942 downloads: open points

kind: decision (owner) and follow-ups
recorded: 2026-11-21

Not run (cannot run on this host): Firefox and Safari engines (the e2e "Firefox" and "Safari" paths are Chromium with that user agent), heap at 5 GiB inside a browser (the engine-level number is flat at about 64 MiB for 1 GiB and 5 GiB; AZ-3952 holds the browser run), the real save dialog (stubbed with an OPFS handle), Forbidden for a non-current revision as a non-admin (unit test only).

Decided 2026-11-22 (owner):
1. Accept `Failed(Network)` for a corrupt server block; AC-5 amended in the done spec.
2. **Add the hash to the public share answer** (`ShareOpen`, contracts part 3, web share download verifies). Scheduled as AZ-4350 (`_docs/02_tasks/todo/AZ-4350_share_open_content_hash.md`).
3. Keep `web.hash_verify_max_ff_safari` a build constant.
4. Keep both row buttons until E-26/c08 lands.

Decisions for the owner (original text):
1. AC-5 says a corrupt block on the volume gives `Failed(HashMismatch)`. The server reader detects its own corruption and aborts the stream, so the client reports `Failed(Network)` ("The download did not finish. Nothing was kept as good."). Nothing is saved or labelled good. `HashMismatch` appears when bytes arrive intact but wrong. Amend AC-5 or accept.
2. A public share link carries no file hash (`GET /public-shares/{token}` has no `content_hash`), so share downloads end `SavedUnverified{NoServerHash}`. Needs a server field to verify them.
3. `web.hash_verify_max_ff_safari` is a build constant, not exposed by `server-info`.
4. The row has two download buttons (the plain link and "Download and check") because AZ-3938 and AZ-3942 disagree on who owns the row action. Merge into one when E-26/c08 lands. The public page button is not wired.

Low: a file replaced between ranged reads ends as HashMismatch (ETag not compared across ranges); OPFS copies are removed only when older than one hour, at the next OPFS download; a failed metadata fetch leaves a row named "Download"; downloads have no cancel button.

Sign-in flake (resolved 2026-11-22): not a sign-in problem. The Files page shows its h1 only until the root folder has loaded, then a breadcrumb takes its place (`FilesPage.svelte`: `trail.length > 0`), so a wait on the h1 alone lost a race with a fast root load. The failing run's page snapshot showed the signed-in page with the breadcrumb. Fixed in the e2e: `filesPage()` in `tests/e2e/support/session.ts` waits for the h1 or the breadcrumb and replaces every h1-only wait. Side note for the web app: after the root loads the Files page has no h1 at all, which a screen-reader user may notice; a design call, not changed.
