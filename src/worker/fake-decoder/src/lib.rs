//! FD, the fake decoder of harness H2 (`09_worker_supervisor/tests.md`): it speaks decoder protocol v1 through the
//! shared codec (`media_shared::decoder_protocol`) on a real Unix socket, answers from a script, records every
//! message it receives, and can send hostile frames and plant hostile output files. The worker crate's in-process
//! tests and the `fake-decoder` binary of the Compose test stack run this same code.
//!
//! FD is a fault and protocol stub only. It never decodes anything: its "outputs" are a few fake bytes, so it must
//! never answer a case that claims a media outcome (those use the real decoder, RD).
//!
//! # Binary
//!
//! `fake-decoder --socket <path> --script <file> --record <file> [--socket-gid <gid>]` binds the socket as the real
//! decoder does (its own copy of 10's socket setup: a stale socket at the path is removed, any other file there is an
//! error, the group is set when given, then mode 0660), removes nothing else, and exits 0 on SIGTERM. A bad
//! argument exits 2; a bad script, record file or socket exits 1 with the reason on stderr.
//!
//! # Script file
//!
//! One JSON object; every field is optional and an unknown field or answer kind is a parse error.
//!
//! ```json
//! {
//!   "hello": {"build_timestamp": 1700000000, "nvenc_probe_ok": true},
//!   "self_check": [{"name": "no_network", "ok": false}, {"name": "no_secrets", "ok": true}],
//!   "probe": {"queue": [{"probe": {"kind": "photo", "container": "jpeg", "moov_first": false,
//!                                  "video": null, "audio": null, "duration_ms": null}}],
//!             "default": {"error": "unsupported"}},
//!   "render_still": {"queue": ["silent", {"image": {"width": 320, "height": 240}}],
//!                    "default": {"error": "corrupt"}},
//!   "transcode": {"default": {"error": {"encoder_failed": {"exit_code": 1}}}},
//!   "idle_outage": {"after_ms": 500, "refuse_ms": 3000}
//! }
//! ```
//!
//! - `hello`: the `Capabilities` fields to change; the others keep [`capabilities`] (protocol 2, build time
//!   1,700,000,000, no NVENC).
//! - `self_check`: the report, in the protocol's own form; default one passing `no_network` item.
//! - `probe`, `render_still`, `transcode`: per request kind, `queue` (optional) is played in order, then `default`
//!   (required when the section is given) answers every later request. Without the section a probe answers
//!   `{"error": "unsupported"}` and a render or transcode `{"error": "corrupt"}`.
//! - `idle_outage`: once, `after_ms` after the first `self_check` answer, the outage of the `outage` answer below.
//!
//! Answers (external tags, as in the protocol):
//!
//! | Form | What FD does |
//! |---|---|
//! | `{"probe": P}` | answers the probe facts `P` (the protocol's `Probe`) |
//! | `{"image": {"width": W, "height": H, "output": O}}` | puts `O` at `out/result.webp`, answers `image_result` |
//! | `{"transcode": {"probe_of_output": P, "output": O}}` | puts `O` at `out/result.mp4`, answers `transcode_result` |
//! | `{"error": E}` | answers the protocol's `DecodeError`, e.g. `"stalled"`, `{"limit_exceeded": {"limit": "wall"}}` |
//! | `"hang"` | reads the request and closes the connection |
//! | `"silent"` | reads the request, sends nothing, ignores `cancel` |
//! | `{"alive_only": {"every_ms": N}}` | sends `alive` every N ms, never answers, ignores `cancel` |
//! | `{"garbage": {"framed": F, "chunks": [C...]}}` | writes the chunks' bytes (behind a length prefix when F) instead of an answer, then ignores everything |
//! | `{"over_time": {"delay_ms": N, "alive_every_ms": A, "answer": X}}` | plays answer X after N ms; sends `alive` every A ms meanwhile when A is given (needed when N exceeds the supervisor's silence timeout); a `cancel` meanwhile is answered `cancelled` |
//! | `{"outage": {"refuse_ms": N}}` | closes every connection and the listener (connects are refused, the socket file stays), binds again after N ms |
//!
//! A garbage chunk is `{"hex": "7bfffe7d"}`, `{"text": "..."}` (its UTF-8 bytes) or
//! `{"repeat": {"byte": 32, "count": 65537}}`; at most 16 MiB in all. An output `O` (default `"file"`) is one of:
//! `"file"` (a regular file of a few fake bytes, which the API's output sniffer refuses, so no FD output can land
//! as a derived file by accident), `{"bytes": {"chunks": [C...]}}` (a regular file of exactly those bytes, for a
//! control case that needs a postable output), `"absent"` (nothing), `{"symlink": {"target": "/path"}}`,
//! `{"hard_link": {"in_file": "source"}}` (a link to that file in the job's `in/`, `st_nlink` 2), `"fifo"`,
//! `{"sparse": {"bytes": N}}` (a sparse file of apparent size N), `"directory"` or
//! `{"swap": {"target": "/path", "bytes": {"chunks": [C...]}, "times": N}}` (ST-09-3: a regular file of those bytes
//! and a symlink to the target beside it, `out/.swap`; after the answer is sent and before the next request is
//! answered, FD exchanges the two names N times with `renameat2(RENAME_EXCHANGE)`, 1 to 1,000,000, stopping at the first
//! error, such as the supervisor clearing `out/`). The `bytes` an answer reports is the size of the regular file
//! behind the output, the apparent size of a sparse one, and 1 otherwise. An answer
//! FD could not send under the protocol's range table (a width of 0, say) is a script error: out-of-range values
//! go in a garbage frame.
//!
//! # Record file
//!
//! Appended, never truncated: one JSON line per received message, written with one unbuffered `write` before the
//! answer, so the line of every message received survives a SIGKILL of FD.
//!
//! ```json
//! {"at_ms":3,"conn":1,"kind":"hello","fields":{"protocol":2}}
//! {"at_ms":4,"conn":1,"kind":"self_check","fields":{"forbidden_targets":["db:5432"]}}
//! {"at_ms":40,"conn":1,"kind":"render_still","job":"0193b2a4-...","out_entries":0,"input":{"bytes":219573,"mode":288,"gid":10003,"hash":"9f2c..."},"fields":{"job_dir":"/scratch/0193b2a4-...","max_edge":1440,"poster_at_ms":null,"params_version":1,"limits":{"cpu":2,"memory_bytes":2147483648,"wall":120000,"stall_timeout":90000,"max_output_bytes":33554432}}}
//! {"at_ms":41,"conn":1,"kind":"cancel","fields":{}}
//! ```
//!
//! `at_ms` is milliseconds since FD started; `conn` counts connections from 1 in accept order; `kind` is the
//! protocol name of the request and `fields` its fields; a job request adds `job` (the last component of
//! `job_dir`, the job id), `out_entries` (entries in the job's `out/` when it arrived) and `input`, the facts of
//! `in/source` then: `{"bytes", "mode", "gid", "hash"}` (permission bits, group, and the file hash in hex, BLAKE3
//! per 2 MiB block rolled up). FD's own events carry a `detail` text instead of `fields`: `invalid` (the client sent
//! a frame the protocol refuses; the connection closes), `fd_error` (FD could not plant an output; the connection
//! closes without an answer, so a case must check that none occurred), `swap_end` (`exchanged D of N`, then the
//! error when one stopped the exchanges early), `input_unreadable` (a job request whose
//! `in/source` FD could not read; that job line has no `input`), `out_unreadable` (its `out/` could not be listed;
//! the line says 0 entries), `outage_start` and `outage_end` (`conn` 0).

mod decoder;
mod forms;
pub mod hostile;
mod input;
mod output;
mod record;
mod script;
mod serve;
mod socket;

pub use decoder::{Config, FakeDecoder, FakeError};
pub use record::{Record, Recorder, Seen};
pub use script::{Answer, IdleOutage, Output, Queue, Script, ScriptError, capabilities};
