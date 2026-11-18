# Leftover: tracker write pending, AZ-3985 to In Testing

kind: tracker
recorded: 2026-11-18
blocked action: `transitionJiraIssue` AZ-3985 `In Testing` (cloudId 1598226f-845f-4705-bcd1-5ed0c82d6119, transitionName `In Testing`)
reason: twice "PreToolUse hook did not respond before its timeout (host client may be unreachable); the tool call was not executed". Batch 93 is committed and pushed (38d9743); only the status is missing.

Replay: run the transition, read the issue back (status `In Testing`), delete this file.
