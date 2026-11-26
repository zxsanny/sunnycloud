# Friction notes

- 2026-11-26 | env | autodev B1.5 | tracker doc had no mcp_server or statuses field, so the session had to probe which Atlassian server reaches the AZ project; four shipped tickets (AZ-3855, AZ-3857, AZ-3946, AZ-4024) were still To Do | cause: implement skill Step 12 transition skipped in batches 99-110, unknown why | fix: added mcp_server and statuses to _docs/00_problem/tracker.md; implement Step 12 should fail the batch when the read-back is not In Testing
