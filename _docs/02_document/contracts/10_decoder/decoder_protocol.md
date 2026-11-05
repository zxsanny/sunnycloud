# Contract: decoder_protocol v1 (framing, parsing and session rules)

**Component**: 10_decoder (module `decoder_protocol` of `media-shared`; server side in 10, client side in 09)
**Producer task**: E-19/c04 — `_docs/02_tasks/todo/B06-04_decoder_protocol_module.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: E-19/c05 (runner socket loop, server side), E-20/c05 (`DecoderClient`, client side), E-19/c15 and E-20/c11 (protocol clients and the fake decoder of the test suites)
**Version**: 1.0.0
**Status**: draft (open points 1-6 below are fixed by the producer task before it is frozen)
**Last Updated**: 2026-11-04

## Purpose

The only channel between the trusted supervisor (09) and the untrusted decoder (10) is this protocol on a Unix socket. Each side treats the other as hostile: the runner because a compromised supervisor must not raise its limits or inject arguments, the supervisor because a compromised decoder can send arbitrary bytes. Both sides therefore share one frame codec and one strict parser, so a field or range rule cannot drift between producer and consumer.

The message types (requests, responses, events, `DecodeError`, `Probe`, `Plan`, `Limits`, `Capabilities`) are defined once in `_docs/02_document/contracts/media-shared/skeleton_types.md` (E-00/c06). This contract adds what the types alone do not fix: framing, size caps, strict parsing and ranges, the session rules and how each side reacts to a violation.

Sources: `components/10_decoder/description.md` §3, §7, §12; `components/09_worker_supervisor/description.md` §2, §3, §7; tests IT-10-1, IT-10-3, IT-10-17, IT-10-19, ST-10-2, IT-09-13, IT-09-15; `tests/spec-gaps.md` G-10-7 (resolved: `ProtocolViolation`).

## Shape

### Transport

| Item | Rule |
|---|---|
| Socket | Unix stream socket at `decoder_socket_path`, created by the runner at start with mode 0660 and group `decoder_gid`; one socket per decoder container |
| Connections | The runner serves one connection at a time. Closing the connection is not a violation |
| Direction | Client to runner: requests. Runner to client: exactly one response per request, plus events while a job runs |

### Frame

| Item | Rule |
|---|---|
| Layout | 4-byte unsigned length prefix, then that many bytes of UTF-8 JSON (one message per frame) |
| Size cap | Payload length at most 65,536 bytes (64 KiB). A larger announced length is a violation and is refused before any payload byte is read or buffered |
| Encoding | Payload must be valid UTF-8 and one JSON value; anything else is a violation |
| Partial frame | A reader never returns a message from an incomplete frame. Bounding the wait is the caller's job (supervisor: `decoder_silence_timeout`; runner: its main loop keeps serving its watchdog while it waits) |

### Messages

| Kind | Members | Notes |
|---|---|---|
| Requests | `hello`, `self_check`, `probe`, `render_still`, `transcode`, `cancel` | Input fields as in the skeleton contract. `probe`, `render_still` and `transcode` are *job requests* |
| Responses | the result type of each request, or a `DecodeError` | One per request; for a job request it is sent only after the runner's kill-all and `work/` removal (10 §3) |
| Events | `Alive`, `Progress{out_time_ms, speed}` | Only while a job request is in flight. `Alive` every `decoder_alive_interval`; `Progress` for each ffmpeg progress line |
| Errors | `DecodeError` variants of the skeleton contract plus `ProtocolViolation` | Variants carry enumerations and numbers only (for example `EncoderFailed{exit_code}`), never free text or child output |

### Strict parsing (both directions)

| Rule | Applies to |
|---|---|
| Unknown field anywhere in a message is a violation | all messages |
| Unknown enumeration value (message kind, media kind, encoder, kill reason, ...) is a violation | all messages |
| Every number is range-checked against the table of open point 4; out of range is a violation | all messages |
| `Plan` is an enumeration-only record; a string where the schema has none is a violation | `transcode` |
| `job_dir` is the only path a request carries; resolving it under `decoder_scratch_root` without symlinks is the runner's job (E-19/c07b), not the parser's | `probe`, `render_still`, `transcode` |
| `limits` values that are valid for their type but above the runner's ceilings are **not** violations: the runner clamps them (IT-10-3, IT-10-12) | `render_still`, `transcode` |

### Session rules

| Situation | Result |
|---|---|
| `hello` with `protocol = 1` | `Capabilities`; `hello` may be repeated on one connection and is answered each time (IT-10-1) |
| `hello` with any other protocol | `ProtocolMismatch`; clean error, the runner stays up |
| A second job request while one runs | `Busy`; the running job continues and completes normally (IT-10-3) |
| `cancel` while a job runs | `Cancelled` after the job's process group is killed and `work/` is removed |
| Connection closed while a job runs | Treated as `cancel` with no reply; the runner then accepts a new connection without restarting (IT-10-19) |
| Violation by the client (oversize, unknown field, out of range, bad UTF-8, out of sequence) | Runner replies `ProtocolViolation` (or closes) and exits within 5 s; Compose restarts it (hostile-class, IT-10-3) |
| Violation by the runner (same classes, seen by the supervisor) | Supervisor drops the connection, marks the slot unhealthy, fails the current job `Retry`, reconnects with backoff and repeats `hello` then `self_check` before the slot claims again (IT-09-13, IT-09-15) |
| Runner exits after a hostile-class outcome or watchdog abort | Client sees end of stream after the typed reply (or none); same reconnect rule as above |

## Invariants

- One frame codec and one parser serve both sides; neither side has a lenient path.
- No frame larger than 64 KiB is ever buffered; an announced length above the cap allocates nothing of that size.
- No message field can become a command-line argument: `Plan` and every request carry enumerations, bounded numbers and the single `job_dir` path.
- Every error crossing the socket is a typed `DecodeError` variant; no child stderr or message text crosses it.
- A protocol violation ends the connection on both sides; a clean `Corrupt`, `Unsupported` or `ProtocolMismatch` does not.

## Non-Goals

- Not covered: the message type definitions themselves (skeleton contract, E-00/c06); what the runner does to execute a job (E-19 executors); the transcode policy (`media-shared::policy`, E-18); hostile output files under `out/` (supervisor `Scratch`, E-20/c04); limits clamping values (runner config, E-19/c05).
- Not covered: more than one connection at a time, or any transport other than the Unix socket.

## Versioning Rules

- `protocol` in `hello` is 1. Any breaking change to framing, a message, a field or a range (rename, removal, type change, tighter range) bumps it, and both images ship together (same release, same host).
- Adding an optional field or a new `DecodeError` variant is still breaking here, because both sides reject unknown fields; it also bumps `protocol`.
- Fixing the open points below before the first implementation is merged is part of 1.0.0 (status moves from draft to frozen).

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| hello-ok | `hello{protocol: 1}` | `Capabilities` reply; repeated 3 times, 3 replies | IT-10-1 |
| hello-mismatch | `hello{protocol: 99}` | `ProtocolMismatch`; connection and runner stay up | IT-10-1 |
| frame-oversize | payload of 65,537 bytes | violation; runner exits within 5 s | IT-10-3, IT-09-13 (64 KiB + 1) |
| prefix-absurd | length prefix of 4 GiB | violation before any payload read; no large allocation | IT-10-3 |
| unknown-field | request or reply with one extra field | violation | IT-10-3, IT-09-13 |
| bad-utf8 | payload with invalid UTF-8 | violation | IT-09-13 |
| range-edge | `render_still.max_edge` of -1, 0 and 100000; reply `width` 0 and 4294967295; negative `duration_ms` | violation | ST-10-2, IT-09-13 |
| unknown-enum | reply with an unknown media kind | violation | IT-09-13 |
| plan-free-text | `transcode` whose `Plan` carries a string field | violation | ST-10-2 |
| limits-above-ceiling | `limits{cpu: 9999, memory_bytes: 1 TiB, wall: 1 day}` | accepted by the parser; clamped by the runner | IT-10-3, IT-10-12 |
| busy | second `render_still` during a running one | `Busy`; first job completes | IT-10-3 |
| socket-loss | client closes during a transcode | job cancelled, no reply; new connection accepted | IT-10-19 |
| stalled-frame | a frame announces a length then stops | no message returned; the supervisor's silence timeout ends the wait | IT-09-13 |

## Open points (fixed by E-19/c04 before freeze)

1. Byte order of the 4-byte length prefix is not stated in any spec.
2. The JSON envelope that tags a message as request, response or event (tag names, where the request kind sits) is not stated.
3. The JSON encoding of durations in `limits` (`wall`, `stall_timeout`) and of `build_timestamp` (tests accept RFC 3339 or epoch) is not stated.
4. Numeric ranges are required ("every number range-checked") but no spec gives the bounds. Known must-reject values: `max_edge` -1, 0, 100000; `width` 0 and 4294967295; negative `duration_ms`. The table of bounds per field is written here by the producer task.
5. "Request out of sequence" is a violation, but the sequence is not defined beyond `Busy` for a second job request; whether `hello`, `self_check` or `probe` while a job runs answer `Busy` or are violations is not stated.
6. Whether the runner refuses or queues a second connection while one is open is not stated.
7. Cross-contract (owners E-18 and E-00/c06): 10 §4 says WebP quality, CRF and preset come from `Params[params_version]`, "never from env", but neither `render_still` nor `Plan` carries a `params_version` or those values. The values themselves live in `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md` ("Output parameters, `params_version` 1"; CRF and preset are its open point 4). Either a field is added to the skeleton types or the decoder holds the `Params` table; this contract cannot be frozen until one is chosen.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial draft contract | decompose Step 2 (B06) |
