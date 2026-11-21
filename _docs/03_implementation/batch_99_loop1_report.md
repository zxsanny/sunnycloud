# Batch 99 (loop 1): MVP build, reconciled against the specs on 2026-11-21

The MVP (roadmap `_docs/02_document/mvp_roadmap.md`) was built outside the batch loop, so these 14 specs stayed in `todo/` with no report and their Jira tickets stayed To Do. This report matches each spec's acceptance criteria (AC) to code and tests (read-only audit by two subagents, verdicts by the parent). A test counts only if it asserts the AC. Nothing was archived and no ticket was moved: an AC that is not done keeps its spec in `todo/`.

| Ticket | Verdict | Remaining scope (own follow-up) |
|---|---|---|
| AZ-3854 ProcessGuard | PARTIAL | stall kill (AC-6) and the ffmpeg progress sink absent; AC-4/5/7/9 asserted with smaller sizes and no timing bounds; no idle-CPU check |
| AZ-3855 containment | PARTIAL | LEAK next-job test (AC-2); seccomp profile, no-new-privileges and the ptrace/environ EPERM test (AC-6); no 1 s bound |
| AZ-3857 ImageExecutor | PARTIAL | orientation matrix 1 to 8, SSIM check, time and 200 MiB memory bounds, ICC/GPS strip proof |
| AZ-3858 probe | PARTIAL | `/etc/passwd` and concat probe cases, latency and memory bounds, full field comparison |
| AZ-3859 video poster | PARTIAL | AC-2 output equality, AC-3 vips argv proof, AC-4 SSIM record and SDR tag |
| AZ-3935 SPA scaffold | PARTIAL | fault-proxy blackbox (AC-3, AC-5), CSP directive and foreign-host check (AC-9), Firefox/WebKit (AC-1) |
| AZ-3936 sign-in, setup | near complete | AC-3 second-visit check, AC-7 IndexedDB/Cache Storage check |
| AZ-3956 tokens, shell | PARTIAL (large) | contrast calculator (AC-5), responsive shell with sidebar/rail/role group/quota meter (AC-7), AC-4/6/8 component tests; roadmap postponed the component library only |
| AZ-3938 file browser | PARTIAL | rename, move, windowing at 10k with the 200-row cap, multi-select, name-truncation test |
| AZ-3939 small-file transfer | PARTIAL | folder upload, 200 in-flight `File` cap with heap test, small-batch cancel |
| AZ-3940 large upload | PARTIAL | 2 s publish, page-holds-no-File check, long-task check, 150 MB heap, 3-engine vectors (not postponed by the roadmap) |
| AZ-3945 thumbnails | done as replaced | polling replaced by pushed `ThumbState` frames (roadmap 5C); the spec text needs the owner's amendment; reduced-motion test missing |
| AZ-3947 render mode | PARTIAL | hostile set (XML, JS, GIF/HTML polyglot) in a real browser, hostile text in panel and notices, session/device-code sentinels |
| AZ-3950 web harness | mostly NOT DONE | the Chromium acceptance smoke exists in `tests/e2e`; the harness (Firefox profile, fault proxy, heap sampler, Safari runner, per-test isolation, seeded-fault probe) and its CI wiring do not |

## Code Review Verdict

Not run for this batch: the commits listed below were each pushed with CI green (run 44886201366). This report is a spec reconciliation, not a new change.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | Specs AZ-3945 (polling) and AZ-3938 (virtual list) describe a form the owner replaced or postponed on 2026-11-20 | AZ-3945, AZ-3938 | the owner amends or splits the specs; until then they stay in `todo/` | unclear |
| 2 | Folder upload and the 200-file in-flight cap are in AZ-3939 but not in the roadmap's postponed list | AZ-3939 AC-6, AC-5 | build after the MVP | clear |

## Commits

b84bbae, e09b28a, 46cf2af, 3e8dbbd, cb7089d, 17143c7, 8adfcef, fb4c227, f09c731, 8e4444b, effeb8f — Loop: 1
