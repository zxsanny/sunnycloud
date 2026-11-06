# Runner socket loop: one connection, busy, events, socket loss and clamping

**Task**: AZ-3853_decoder_runner_socket_loop
**Name**: Decoder runner socket loop
**Description**: The `decoder-runner` process: typed config, the Unix socket server of decoder protocol v1 with one connection and one job at a time, `Alive`/`Progress` events, connection loss as cancel, and every requested limit clamped to the runner's own ceilings
**Complexity**: 3 points
**Dependencies**: AZ-3852_decoder_protocol_module, AZ-3763_sc_config, AZ-3764_sc_error, AZ-3786_sc_observability, AZ-3652_initial_structure
**Component**: 10_decoder
**Tracker**: AZ-3853
**Epic**: AZ-3634 (E-19)
**Ref**: E-19/c05

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/10_decoder/decoder_protocol.md` — framing, parser and session rules, produced by E-19/c04.
- `_docs/02_document/contracts/shared_config/sc_config.md` — typed decoder section, produced by E-01/c01.

## Problem

The decoder's only channel is a socket shared with its supervisor (`network_mode: none`). The runner must serve exactly one job at a time, keep the supervisor informed while a job runs, stop work when its supervisor dies (a dead supervisor must not leave a CPU-burning decoder, AC-71), and never let a compromised supervisor raise the limits (AC-25).

## Outcome

- A runner that starts from its typed config, listens on its socket and dispatches each request to the job executors.
- One request in flight; `Busy`, `Alive`, `Progress`, `cancel` and connection loss behave as the contract states.
- The limits a job runs with are never above the runner's ceilings.

## Scope

### Included
- Typed decoder config section (helper 01 "Decoder (10)" and "Limits" rows) bound at start; an invalid value refuses start with every problem listed; the section holds no secret.
- Socket at `decoder_socket_path`, mode 0660, group `decoder_gid`; one connection at a time; a new connection accepted after the previous one closes.
- Dispatch of `hello` (protocol check; capability values from E-19/c12), `self_check`, `probe`, `render_still`, `transcode`, `cancel` through the contract's session rules.
- `Alive` every `decoder_alive_interval` while a job runs; `Progress{out_time_ms, speed}` forwarded from the executors.
- Connection loss while a job runs starts the job's cancel path with no reply.
- A client protocol violation: typed reply (or close), then the runner exits within 5 s.
- Clamping of `cpu`, `memory_bytes`, `wall`, `stall_timeout`, `max_output_bytes` to the photo or video ceilings by job kind.
- JSON logging with `component=decoder-runner` (fields `job_id`, `kind`, `profile`, kill reason, exit code, durations; no user paths) and the informational `health` subcommand.

### Excluded
- Killing processes and removing `work/` (E-19/c06, E-19/c07a), safe-open of `in/source` and the self-abort watchdog (E-19/c07b), capability values and self-check items (E-19/c12), the executors (E-19/c08 to c11).

## Acceptance Criteria

**AC-1: Socket and single connection**
Given a runner started with `decoder_socket_path` and `decoder_gid`
When the socket is inspected, a client connects, disconnects and a second client connects
Then the socket has mode 0660 and that group, both clients are served one after the other, and the runner did not restart in between (IT-10-19 (c))

**AC-2: Protocol version check (IT-10-1)**
Given an idle connection
When `hello{protocol: 99}` and then `hello{protocol: 1}` three times are sent
Then the first answers `ProtocolMismatch`, the runner stays up, and each later `hello` is answered

**AC-3: One job in flight (IT-10-3)**
Given a job request in flight
When a second job request arrives
Then it is answered `Busy` and the first job's reply is unaffected

**AC-4: Events while a job runs (PT-10-3)**
Given `decoder_alive_interval = 500ms` and a job lasting 3 s
When the client reads the stream
Then `Alive` arrives every 500 ms (production 5 s +/- 0.5 s, failure above 6 s), each executor progress line arrives as one `Progress`, and no event is sent while idle

**AC-5: Cancel and connection loss (IT-10-19, AC-71)**
Given a job in flight
When the client sends `cancel`, and in a second run closes the connection instead
Then the first gets `Cancelled` within 2 s, the second gets no reply, both runs call the job's cancel path, and a new connection is accepted afterwards

**AC-6: Client violation ends the runner (IT-10-3)**
Given an open connection
When a frame of 65,537 bytes, a frame with an unknown field, or a 4 GiB length prefix is sent
Then the runner replies `ProtocolViolation` or closes, and exits within 5 s

**AC-7: Limits clamped (AC-25, IT-10-12)**
Given ceilings `photo_job_timeout = 3s`, `photo_memory = 128MiB`, `photo_cpu = 2`
When a photo request asks for `wall = 3600s`, `memory_bytes = 64GiB`, `cpu = 64`, `max_output_bytes = 1TiB`
Then the job runs with 3 s, 128 MiB, 2 CPU and `decoder_max_output_photo`, and the same holds for video with the video ceilings

**AC-8: Config refusal**
Given a decoder config with an invalid duration and an unknown `SUNNYCLOUD_*` variable
When the runner starts
Then it refuses to start and lists both problems

## Non-Functional Requirements

**Performance**
- Idle CPU under 0.5% (failure 2%); memory growth over 100 jobs under 20 MiB (failure 100 MiB) (PT-10-3, measured in E-19/c16).

**Reliability**
- The main loop never blocks the self-abort watchdog's heartbeat while it waits for a frame (contract: partial frame rule).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Session state with a scripted job | `Busy` for the second job request |
| AC-5 | Cancel and loss with a scripted job | Cancel path called; `Cancelled` / no reply |
| AC-7 | Clamping table for photo and video | Effective limits = min(request, ceiling) |
| AC-8 | Config validation | All problems listed |
| AC-4 | `Alive` timer and progress forwarding with a scripted job (500 ms interval) | One `Alive` per interval while busy; none idle; one `Progress` per line |

## Blackbox Tests

Process-level cases run the runner binary with the protocol client. Cases that need a real running job (busy with a real render, cancel of a real transcode, clamped kill timing) run in E-19/c15 once the executors exist: IT-10-3 busy part, IT-10-12, IT-10-19, PT-10-3.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Runner binary, test socket path and group | Socket mode, sequential connections | 0660, group set; both served, no restart | IT-10-19 (c) |
| AC-2 | Idle runner | IT-10-1 version cases | Mismatch clean; repeated hello answered | AC-25 |
| AC-6 | Idle runner | IT-10-3 frame cases | Violation reply or close; exit within 5 s | AC-25 |
| AC-8 | Invalid config | Start | Refused, problems listed | |

## Constraints

- No clock helper, no database, no HTTP client, no `contracts`, never `media-shared::policy` (10 §1); timers use the monotonic standard clock.
- The decoder holds no secret; its config section has no secret key.

## Risks & Mitigation

**Risk 1: Event writes block the job**
- *Risk*: a slow or stuck reader stalls the runner while it writes `Progress`.
- *Mitigation*: events are bounded and a supervisor that stops reading is ended by its own silence timeout (E-20/c07).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| JSON logging means using E-09's `sc_observability`, which imports `sc_db` (sqlx); 10 §1 allows only helpers config and error. Linking it pulls database code into the untrusted image (the `contracts` edge is resolved, next row) | E-09 and E-00 owners (crate graph) | open | High |
| Resolved by owner decision 2026-11-05: `HealthContributor` and `ReadinessCheck` live in `sc_observability`, `contracts` re-exports them, and `sc_observability` no longer depends on `contracts`, so linking it brings no `contracts` into the decoder image | Owner | resolved | High |
| The `health` subcommand is named (containerization, 10 §8 "informational") but what it checks is not stated, and the socket serves one connection at a time | E-19 owner | open | Low |
