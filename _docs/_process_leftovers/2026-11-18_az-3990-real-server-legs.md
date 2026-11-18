# Leftover: AZ-3990 and AZ-3991 real-server legs

kind: test (NOT-RUN(NO-SERVICE))
recorded: 2026-11-18
blocked action: AC-78 against the real server with two processes (IT-17-3, FT-P-76: 100 assets, exactly 100 server files, same `client_key` per asset) and the AC-5 blackbox (kill after the commit acknowledgement, restart, `NothingToSend`, one server file)
reason: needs a running server and database in the test stack (E-31); the unit tests use a fake server that is idempotent by `client_key` and `commit_id`, which proves the client's keys and batch order, not the server's behaviour

Replay: when the E-31 stack serves the API, run the two cases from `tests/` against it and delete this file.

Also AZ-3991: `set_server` against a real https server (server-info answer, `min_client_version` above ours) and `cancel` through a real `HttpSyncClient`; the unit tests cover those rows at the mapping boundary with fakes.

Also AZ-3997: the batch retry after `parent_not_found` / `parent_deleted` (a second `stage_small` and commit for the same `staged_key`) and a stored large-upload session after the camera folder was recreated, against the real server.
