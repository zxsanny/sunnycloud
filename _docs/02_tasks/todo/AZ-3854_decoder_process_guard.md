# ProcessGuard: argv-only children with limits, RSS polling and wall/stall kill

**Task**: AZ-3854_decoder_process_guard
**Name**: Decoder ProcessGuard
**Description**: Start every `vips*`/`ff*` child from an argv vector with a cleared environment, its own process group and fixed rlimits, poll its memory, and kill the group on wall time, memory or missing progress, returning a typed outcome
**Complexity**: 5 points
**Dependencies**: AZ-3853_decoder_runner_socket_loop, AZ-3849_decoder_s5_whitelist_hdr_spike, AZ-3763_sc_config, AZ-3652_initial_structure
**Component**: 10_decoder
**Tracker**: AZ-3854
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c06

## Problem

The media tools parse attacker-controlled files and may hang, allocate without bound, write huge outputs or be exploited (R01). A shell or an inherited environment would turn a crafted name into a command. The limits of AC-25 (photos 2 CPU / 2 GiB / 120 s; video 4 CPU / 4 GiB / 40 min) and the 90 s stall kill of AC-45 must hold for every child, and the outcome must be a typed value the runner can classify, never parsed message text.

## Outcome

- Every child runs from an argv vector, with only the caller's environment allowlist, null stdin and its own process group.
- Wall, memory and stall limits kill the whole group and come back as `Killed{wall|memory|stall}`; a normal end is `Exited{code}`.
- Bounded, sanitised stderr is available for logs; nothing keys on its text.

## Scope

### Included
- Spawn: argv vector, never a shell; environment cleared then set to the caller's allowlist; stdin null; own process group.
- rlimits: `RLIMIT_CORE = 0`, `RLIMIT_NOFILE`, `RLIMIT_FSIZE = max_output_bytes`, `RLIMIT_CPU` as backstop; **no** `RLIMIT_AS`.
- Group RSS polled every `decoder_memory_poll_interval`; kill above `memory_bytes`.
- Wall kill at `wall`; stall kill when no progress line arrives for `stall_timeout` (ffmpeg only), using the progress sink fixed by gate S5 (E-19/c01).
- Outcome `Exited{code}` or `Killed{reason: wall|memory|stall}`; a child ended by another signal or by `RLIMIT_FSIZE` is reported as such so executors and containment can classify it.
- Progress lines handed to the caller for `Progress` events; stderr read with a bound, a 2 KiB sanitised tail (no control characters) kept for logs.

### Excluded
- Killing descendants outside the group, `work/` removal and hostile-class exit (E-19/c07a); thread-cap values in argv or env (set by the executors E-19/c08, E-19/c11 from the clamped cpu limit).

## Acceptance Criteria

**AC-1: No shell, fixed arguments (ST-10-2)**
Given a job directory name containing spaces, `;`, `$(x)` and quotes
When a test child is started with it as one argument
Then the child receives it as exactly one argument and no `/bin/sh` process is created

**AC-2: Cleared environment and null stdin**
Given the runner's own environment has extra variables
When a child is started with an allowlist of two variables
Then the child's environment is exactly those two and its stdin is at end-of-file

**AC-3: rlimits (IT-10-12)**
Given a running child
When its limits are read from the process table
Then core size is 0, file size equals `max_output_bytes`, open files and CPU time are set, and no address-space limit is set

**AC-4: Wall kill**
Given `wall = 3s` and a child that sleeps
When it runs
Then the group is killed and `Killed{wall}` returns within 3 s + 1 s

**AC-5: Memory kill**
Given `memory_bytes = 128MiB`, poll interval 100 ms and a child that allocates 512 MiB
When it runs
Then `Killed{memory}` returns within 1 s of crossing 128 MiB

**AC-6: Stall kill and no false kill (AC-45, IT-10-13)**
Given `stall_timeout = 3s`
When a child prints progress lines and then stops, and in a second run keeps printing a line every second for 10 s
Then the first is `Killed{stall}` within 3 s + 1 s of its last line, and the second exits normally with no kill

**AC-7: Output size limit (IT-10-12)**
Given `max_output_bytes = 1 MiB`
When a child writes 1 GiB
Then the write is stopped by the file-size limit and the outcome says so

**AC-8: Typed outcome only**
Given a child exiting with code 1 while printing "Killed" on stderr, and a child ended by SIGSEGV
When each finishes
Then the first is `Exited{code: 1}`, the second is reported as ended by a signal, and neither result depends on stderr text

**AC-9: Bounded stderr (ST-10-4)**
Given a child writing 1 MiB of stderr with ANSI escapes and newlines
When it is killed
Then the retained tail is at most 2,048 bytes with no control characters

## Non-Functional Requirements

**Performance**
- Polling adds no more than one RSS read per poll interval; the guard does not raise idle CPU above PT-10-3's 0.5%.

**Reliability**
- The cgroup limit of the container stays the hard memory stop (10 risk 2); RSS polling is the early, typed stop.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Argument passing with hostile names | One argument; no shell |
| AC-2 | Environment and stdin of a test child | Allowlist only; stdin EOF |
| AC-3 | Limits table of a running test child | Values as specified; no `RLIMIT_AS` |
| AC-4 | Sleeping test child | `Killed{wall}` on time |
| AC-5 | Allocating test child | `Killed{memory}` within 1 s |
| AC-6 | Progress-printing test children | Stall kill; no false kill |
| AC-7 | Writing test child | File-size stop reported |
| AC-8 | Exit-code and signal children | Typed outcome, no text matching |
| AC-9 | Noisy stderr child | Tail <= 2,048 bytes, sanitised |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-3, AC-4, AC-5, AC-7 | Stub markers (E-19/c15) | IT-10-12 | Limits hold, `LimitExceeded` variants | NFT-RES-LIM-04, AC-25 |
| AC-6 | Real ffmpeg on DS-05 base, SIGSTOP | IT-10-13 (a), (b) | `Stalled` on time; none while progressing | AC-45, FT-N-06, NFT-RES-19 |
| AC-9 | Stub with 1 MiB stderr | ST-10-4 | Tail bounded and sanitised | AC-25 |

## Constraints

- Linux only; the guard is used only inside the decoder container.
- No `RLIMIT_AS` (breaks NVENC below 4 GiB, fact card).

## Risks & Mitigation

**Risk 1: RSS polling misses a spike**
- *Risk*: a fast allocation passes the limit between polls.
- *Mitigation*: the container cgroup limit stops it; the supervisor sees socket loss and treats it as transient (10 risk 2).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The stall rule depends on the progress sink working under `-protocol_whitelist file` (10 risk 5); fixed by E-19/c01 before this task's ffmpeg cases | E-19 owner | open | Low |
