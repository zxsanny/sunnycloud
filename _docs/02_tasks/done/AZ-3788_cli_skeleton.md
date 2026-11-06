# `sunnycloud` CLI skeleton: command tree, exit codes, `--json`, confirmation guard, secret writer, lazy context

**Task**: AZ-3788_cli_skeleton
**Name**: CLI skeleton
**Description**: The binary's command tree with the final names, one exit-code mapping, the `--json` envelope, the destructive-command guard, the one-time secret writer, cooperative Ctrl-C and a lazily built service context, plus the command test harness every command task uses
**Complexity**: 3 points
**Dependencies**: AZ-3763_sc_config, AZ-3764_sc_error, AZ-3786_sc_observability, AZ-3652_initial_structure
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3788
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c02

## Problem

Every operator command does the same delivery work: parse arguments, confirm a destructive action, print text or JSON, print a secret once, map the outcome to an exit code. Without one place for it, commands map outcomes differently, a token ends up in a log, and a scripted run deletes an account without confirmation. Scripts, Compose and the test harness read exit codes and JSON, so the shape must be stable.

## Outcome

- Every command of `canonical-names.md` "Operator commands" parses; the old names `key rewrap`, `media requeue` and a top-level `restore` are usage errors.
- One mapping from outcome classes to exit codes: 0 success, 1 warnings only, 2 failed, refused or findings, 3 usage or configuration (130 for SIGINT in `serve` is applied by E-22/c05).
- `--json` prints exactly one object `{result, code, data}`.
- Destructive commands confirm: a non-TTY without `--yes` exits 3 with no business call; a TTY gets a prompt that names the target.
- One-time secrets go to stdout once through a writer that bypasses the logger.
- A service context that builds each service the first time a command asks for it, once per process.
- A command test harness: run any command through the binary's entry point with recording owner ports, capture exit code, stdout, stderr and the calls made.

## Scope

### Included
- The command tree (names, arguments and help text of every command in the command-surface contract); command handlers that are empty until their task lands.
- The outcome-to-exit-code mapping and the `--json` writer; human text to stdout, progress and diagnostics to stderr.
- The confirmation guard with `cli_confirm_required` (default on) and the `--yes` flag.
- The secret writer.
- The lazy service context mechanism (builders are added by E-22/c03).
- A SIGINT handler that triggers the cancellation token passed to long owner operations.
- One structured log line per command when `log = json` (`component=cli`, `command`, `exit_code`, duration), with no secret argument and no user file path.
- The command test harness.
- The contract file `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md`.

### Excluded
- Each command's business call and result mapping (E-22/c07..c15), `serve` (E-22/c05), building real services (E-22/c03), per-command configuration binding (E-22/c04).
- The source grep gates (no SQL, no blob paths): ST-12-3 in E-22/c17; the `sqlx` dependency check is already in E-00's crate-graph test.

## Acceptance Criteria

**AC-1: Final command names**
Given the binary
When each command of the command-surface contract is run with `--help`, and `key rewrap`, `media requeue` and `restore` are run
Then every listed command is recognized, and the three old names exit 3 with usage on stderr and no business call

**AC-2: Usage errors (IT-12-1)**
Given an unknown command and a command with a missing required argument
When each runs
Then each exits 3, prints usage on stderr, builds no service and makes no business call

**AC-3: Exit-code mapping**
Given a command whose recording port returns success, a warnings-only result, an owner refusal (`LastAdmin`) and an unexpected failure
When the command runs for each
Then the exit codes are 0, 1, 2 and 2; a configuration error exits 3

**AC-4: One JSON object (IT-12-1)**
Given `--json` on a successful command and on a command whose port refuses with `LastAdmin`
When each runs
Then stdout holds exactly one JSON object with the keys `result`, `code` and `data`; for the refusal `code` is `last_admin`; text mode prints the same facts; diagnostics stay on stderr

**AC-5: Confirmation guard (ST-12-2, AC-27)**
Given a destructive command and stdin that is not a TTY
When it runs without `--yes`, and then with `--yes`
Then the first exits 3 with zero business calls, the second makes exactly one call; on a TTY the prompt names the target and only a confirming answer makes the call

**AC-6: Secret printed once (ST-12-1, NFT-SEC-38)**
Given a command whose result carries a one-time token, `log = json`, and stdout and stderr captured apart
When it runs in text mode and in `--json` mode
Then the token appears exactly once on stdout in each mode, and 0 times in stderr and in log lines

**AC-7: Lazy context**
Given a context whose database builder fails when called
When `version` runs, and then a command asks for the same service twice
Then `version` succeeds without calling the failing builder, and the twice-requested service is built once

**AC-8: Cooperative Ctrl-C (IT-12-19)**
Given a long command whose recording port waits on the cancellation token
When SIGINT arrives
Then the token is triggered at once, the process never exits before the port has returned, and with a port that honours the token the process ends within 5 s of the signal

**AC-9: Command log line**
Given `log = json`
When a command with a secret file argument runs
Then exactly one `component=cli` line is logged with `command`, `exit_code` and duration, and it contains no secret value and no user file path

## Non-Functional Requirements

**Reliability**
- Coverage of the exit-code mapping, the confirmation guard and the secret writer at least 90% (quality thresholds).

**Security**
- The binary opens no network listener outside `serve`.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Command tree against the contract table | Every name present; old names rejected |
| AC-3 | Outcome-to-exit table | 0/1/2/2/3 as stated |
| AC-4 | JSON writer | One object, three keys, owner code |
| AC-5 | Guard with TTY and non-TTY stdin | No call without `--yes` on non-TTY; prompt names the target |
| AC-6 | Secret writer with a capturing logger | Token once on stdout, never in the log |
| AC-7 | Lazy builder counts | Unused builder not called; shared builder called once |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2, AC-4 | Harness, recording ports | Unknown command, missing argument, `--json` refusal (IT-12-1 part) | Exit 3 no call; one JSON object with `last_admin` | |
| AC-5 | Harness, stdin `/dev/null` | Destructive command without and with `--yes` (ST-12-2 harness part) | Exit 3, 0 calls; then 1 call | AC-27 |
| AC-6 | Harness, `log = json` | Token-issuing recording port (IT-12-19 part) | Token once on stdout only | NFT-SEC-38 |
| AC-8 | Harness, blocking port | SIGINT | Token triggered; exit after the port returns, within 5 s | |

## Constraints

- Thin delivery layer: parse, one business call, print, map. No business rule.
- Error codes come from helper 02's code table; this task adds no wire code of its own.
- Commands run as `docker compose exec|run api sunnycloud ...`.

## Risks & Mitigation

**Risk 1: A secret leaks through the logger**
- *Risk*: a later command prints its token with the normal logger.
- *Mitigation*: the secret writer is the only path for one-time secrets; AC-6 and ST-12-1 (E-22/c17) search every stream.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The value set of `result` and the `code` on success are not stated (contract open point 1) | E-22 owner | open | Low |
| With `--json`, whether a one-time token sits inside `data` or outside the object is not stated; AC-6 only requires "once on stdout" (contract open point 2) | E-22 owner | open | Low |
| Exit code when SIGINT cancels a long command other than `serve` is not stated (G-12-7 covers `serve` only; IT-12-19 records it) | E-22 owner | open | Low |
| What `cli_confirm_required = off` changes is not stated (default on is) | E-22 owner | open | Low |
| Stable codes for operator-only outcomes (`SetupNotAvailable`, `BeyondRetention`, key-check outcomes, migration outcomes) are not in helper 02's code table (contract open point 3) | E-02 / E-22 owner | open | Low |

## Contract

This task produces/implements the contract at `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
