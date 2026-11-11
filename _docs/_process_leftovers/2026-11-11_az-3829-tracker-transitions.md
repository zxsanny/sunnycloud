# Leftover: AZ-3829 Jira transitions (tracker unavailable)

kind: tracker
blocked action: AZ-3829 `To Do` -> `In Progress` -> `In Testing` in project AZ on cloud `1598226f-845f-4705-bcd1-5ed0c82d6119` (tracker doc `_docs/00_problem/tracker.md`)
reason: `getJiraIssue` failed twice in a row on 2026-11-11 with "PreToolUse hook did not respond before its timeout (host client may be unreachable)"; nothing reached Jira, so no state was read or written

Replay: read back AZ-3829 with `getJiraIssue`, transition it to `In Progress` if it is still `To Do`, then to `In Testing`, and read it back again. The work is committed as batch 35 (`[AZ-3829]`). Delete this file after the read-back shows `In Testing`.
