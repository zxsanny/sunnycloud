# Safari S12 run on the owner Mac (AZ-3931)

**Timestamp**: 2026-11-14
**Blocked action**: measure candidates A2 and B2 in Safari at 1 GiB and 2 GiB, plus one corrupt run each (IT-16-8, gate S12)
**Reason**: needs the owner Mac (H-MAC); the agent has no macOS host
**Owner decision**: 2026-11-14, option (a) — measure on the Mac
**Replay**: follow `_docs/00_research/s12_download_spike/README.md`; record the Safari row in the AZ-3931 results and in IT-16-8 (`components/16_web_app/tests.md`) and the IT-16-8 row of `tests/spec-gaps.md`; then delete this leftover.
**Until then**: E-26/c12 builds the labelled unverified path for Safari.
