# A /proc/<pid>/stat that did not parse, once in CI

- Seen: CI run 45130998088 (commit 7c802c6), `rust-test (6)`: `decoder-runner process::tests::exit_code_and_signal_are_typed_whatever_stderr_says`
  failed in 0.03 s with `guard run: InvalidData "unreadable /proc/<pid>/stat"`. The case runs `kill -SEGV $$` and `exit 1` under the guard; the memory poll
  scans `/proc` every 20 ms. The change under test (AZ-3861) does not touch the guard; the same test passed locally in 4 full runs.
- Not reproduced: a local stress scan against crashing children was too slow to run (each SEGV goes through the host's crash handling).
- What is known: the read succeeded (an ESRCH or ENOENT read is already "process gone") and `parse_stat` returned `None`: an empty or short text.
- Done: the error now carries the pid and the first 200 characters of the text (`proc_table.rs`), so the next occurrence says what the file held.
- Next: when it recurs, read the text in the error; if it is an empty or short `stat` of an ending process, treat it as "process gone" in `read_stat` with a test that feeds that text.
