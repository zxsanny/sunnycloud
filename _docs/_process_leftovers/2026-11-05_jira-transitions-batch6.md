# Pending Jira writes (batch 6)

Timestamp: 2026-11-05
Blocked action: transition four issues to `In Testing` after commit 2b2b063.
Reason: Atlassian MCP returned `Tool call failed: 404` for `transitionJiraIssue` (twice for AZ-3771) and for a read `getJiraIssue` on AZ-3771. Earlier calls in this session worked (batch 5, minutes before). Retried once as the tracker rule requires; still failing.

Replay payload (cloudId 1598226f-845f-4705-bcd1-5ed0c82d6119, `transitionName: "In Testing"`; the four tickets are already `In Progress`):

- AZ-3771 (media-shared skeleton)
- AZ-3783 (sc_db Migrator and accounts base)
- AZ-3955 (sync-core-proto)
- AZ-3914 (IPC server; AC-2 Windows pipe NOT RUN)

Read back each issue after the transition; expected status `In Testing`.
