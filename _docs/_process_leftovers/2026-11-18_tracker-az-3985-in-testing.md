# Leftover: tracker writes pending, AZ-3985, AZ-3989, AZ-3990 and AZ-3991 to In Testing

kind: tracker
recorded: 2026-11-18
blocked action: `transitionJiraIssue` AZ-3985 (batch 93, 38d9743) and AZ-3989 (batch 94, 41945d1) and AZ-3990 (batch 95, 3acbdaf) and AZ-3991 (batch 96) to `In Testing` (cloudId 1598226f-845f-4705-bcd1-5ed0c82d6119, transitionName `In Testing`)
reason: twice "PreToolUse hook did not respond before its timeout (host client may be unreachable); the tool call was not executed". Both batches are committed and pushed; only the status is missing.

Replay: run all four transitions, read each issue back (status `In Testing`), delete this file.
