# AZ-3942 downloads: open points

kind: decision (owner) and follow-ups
recorded: 2026-11-21

Not run (cannot run on this host): Firefox and Safari engines (the e2e "Firefox" and "Safari" paths are Chromium with that user agent), heap at 5 GiB inside a browser (the engine-level number is flat at about 64 MiB for 1 GiB and 5 GiB; AZ-3952 holds the browser run), the real save dialog (stubbed with an OPFS handle), Forbidden for a non-current revision as a non-admin (unit test only).

Decisions for the owner:
1. AC-5 says a corrupt block on the volume gives `Failed(HashMismatch)`. The server reader detects its own corruption and aborts the stream, so the client reports `Failed(Network)` ("The download did not finish. Nothing was kept as good."). Nothing is saved or labelled good. `HashMismatch` appears when bytes arrive intact but wrong. Amend AC-5 or accept.
2. A public share link carries no file hash (`GET /public-shares/{token}` has no `content_hash`), so share downloads end `SavedUnverified{NoServerHash}`. Needs a server field to verify them.
3. `web.hash_verify_max_ff_safari` is a build constant, not exposed by `server-info`.
4. The row has two download buttons (the plain link and "Download and check") because AZ-3938 and AZ-3942 disagree on who owns the row action. Merge into one when E-26/c08 lands. The public page button is not wired.

Low: a file replaced between ranged reads ends as HashMismatch (ETag not compared across ranges); OPFS copies are removed only when older than one hour, at the next OPFS download; a failed metadata fetch leaves a row named "Download"; downloads have no cancel button.

Flaky, cause not found: in one of three full e2e runs on fresh stacks (same code), the sign-in helper failed twice ("Files" heading not visible after the mock IdP login) at the first sign-in of web-download and of web-resume; the other two runs passed 37 of 37. The API log of the failing run was lost. If it comes back, keep the logs (`$ACC_DATA/run/*.log`) before `acceptance-down.sh`.
