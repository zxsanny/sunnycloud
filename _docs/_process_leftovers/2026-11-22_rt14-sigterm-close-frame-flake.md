# rt14: the SIGTERM close frame was not seen once in CI

Observed: CI run 45019443148, job `rust-test (2)`, `sunnycloud-server::pipeline_realtime rt14_sigterm_closes_the_sockets_with_1001_before_the_grace_ends`
failed after 0.77 s: `socket.close_code()` returned `None` where the test expects `Some(1001)` (pipeline_realtime.rs:365).

Evidence: the test passed in every other CI run of 2026-11-21 and 2026-11-22 (about 12 runs) and 25 of 25 local runs, also
beside another user's load (host load average 30). Not reproduced, so the cause is not known.

Hypothesis, unconfirmed: the server process exits right after the close frame is written, and a TCP reset from the exit
can discard a frame the client has not read yet; or the test's fixed 300 ms wait before SIGTERM leaves the socket in a
state a loaded runner reaches late. A second occurrence should be investigated by capturing the close frame and the exit
timing in the failing run (log the instant of the close write and of the process exit), not by retrying or loosening the
assertion.

Next step on a second occurrence: instrument, then decide between a server fix (flush and half-close before exit) and a
test change. Do not add the test to a retry list.
