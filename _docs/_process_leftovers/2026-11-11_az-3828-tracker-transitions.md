# Leftover: AZ-3828 Jira transitions (tracker unavailable)

kind: tracker
blocked action: AZ-3828 `To Do` -> `In Progress` in project AZ on cloud `1598226f-845f-4705-bcd1-5ed0c82d6119` (tracker doc `_docs/00_problem/tracker.md`); `In Testing` only after the last operation (the device approval lookup) is registered
reason: Jira calls failed in this session with "PreToolUse hook did not respond before its timeout (host client may be unreachable)" (see `2026-11-11_az-3829-tracker-transitions.md`); `getJiraIssue` for AZ-3828 was tried once after batch 36's gate and failed the same way

Replay: read back AZ-3828 with `getJiraIssue`, transition it to `In Progress` if it is `To Do`, and read it back again. The work is committed as batch 36 (`[AZ-3828]`), 25 of 26 operations. Delete this file once AZ-3828 reaches `In Testing`.
