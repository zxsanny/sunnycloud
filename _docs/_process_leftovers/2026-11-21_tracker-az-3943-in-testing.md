# Leftover: tracker write pending, AZ-3943 to In Testing

kind: tracker
recorded: 2026-11-21

blocked action: `transitionJiraIssue` AZ-3943 (commit 67e06e0) to `In Testing` (cloudId 1598226f-845f-4705-bcd1-5ed0c82d6119, transitionName `In Testing`)
reason: twice "PreToolUse hook did not respond before its timeout (host client may be unreachable). The tool call was not executed". The batch is committed and pushed; only the status is missing. AZ-3943 is `In Progress` in Jira.

Replay: run the transition, read the issue back (status `In Testing`), delete this file.
