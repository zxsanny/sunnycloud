# Helper: Names (validation, name_key, hostile-name rules)

**Crate**: `sc_names` (pure, no I/O; also built for WASM for optional web preflight). **Used by**: 02_tree (create, rename, validation, `name_key`, server-assigned conflict-copy names), 03_transfer (upload name check), 12_operator_cli (restore by path), 13_sync_core (path-to-name splitting), 14_desktop_daemon (portability check, escaping), 17_android_app (`mobile_backup` file names). Shared corpus file `names_corpus.json` is consumed by server, daemon and web tests.

## Purpose
One definition of "a legal name", of the case-insensitive key and of what a client does with a name its OS cannot hold. The server accepts everything except a short list; clients carry the portability burden.

## Interface

| Operation | Input | Output | Errors |
|-----------|-------|--------|--------|
| `validate_name` | `&str` | `ValidName` | `NameError::{Empty, TooLong(bytes), Dot, DotDot, ContainsSlash, ContainsNul, ControlChar, BidiControl}` |
| `name_key` | `&ValidName` | `NameKey` (NFC, then Unicode full case folding) | none |
| `NAME_KEY_VERSION` | const | pinned Unicode version id stored in `nodes.name_key_version` | none |
| `portable_check` | `&ValidName`, `FsProfile` | `Portable::Ok` or `Portable::NeedsEscape(Vec<Reason>)` | none |
| `escape_name` | `&ValidName`, `FsProfile`, `existing: &dyn NameLookup` | `EscapedName` (deterministic, collision-free in the folder) | none |
| `split_path` | `&str` (relative, `/` separated) | `Vec<ValidName>` | `NameError` for any component |
| `conflict_copy_name` | `&ValidName` (the contested name), `at: Timestamp`, `device_label: &str`, `taken: &dyn NameLookup` | `ValidName` (`stem (conflict copy YYYY-MM-DD HH-MM device).ext`, UTC, no collision in the folder) | none |

`FsProfile` = `WindowsNtfs`, `PosixCaseSensitive`, `CaseInsensitivePreserving` (APFS, HFS+, exFAT). Profile detection is done by 14_desktop_daemon per sync root (a probe file test), not here.

## Server rules (AC-37, AC-42, gap 4)
- Reject a whole name equal to `.` or `..`, any name containing `/`, NUL, a control character (Unicode Cc) or a bidi-control character (U+061C, U+200E, U+200F, U+202A to U+202E, U+2066 to U+2069). Names are at most 255 bytes of UTF-8 (assumption A1). Everything else is accepted: reserved Windows names, trailing dots and spaces, backslash, colon, case variants, leading dashes, emoji.
- `name_key` = NFC then full case folding; computed in Rust only (never in SQL), stored with `name_key_version`; `COLLATE "C"` makes uniqueness byte-exact.

## Conflict-copy names (server-assigned)
`conflict_copy_name` is the one function that builds the name of a conflict copy; 02 calls it inside the commit transaction (a stale-base commit, or a new-file name collision), so the server decides and the client never retries with another name. Format `stem (conflict copy YYYY-MM-DD HH-MM device).ext`: time in UTC from the injected clock, `device` is the label of the committing device with `/`, NUL, control and bidi characters replaced by `_`, the extension (text after the last dot, if the name has a stem) is kept, and the stem is truncated on a character boundary so the whole name (including the label, which is cut first if needed) fits 255 bytes. If the result is taken in the folder, ` (2)`, ` (3)` is inserted before the extension. The result always passes `validate_name`.

## Client portability reasons (`Reason`)
Windows reserved device names (CON, PRN, AUX, NUL, COM1-9, LPT1-9, with or without extension), trailing dot or space, characters `< > : " | ? * \`, alternate-data-stream syntax (`name:stream`), length over the platform limit, lone surrogates; macOS/Windows case collisions with an existing sibling; NFC/NFD collisions on APFS; on Linux only NUL and `/` are impossible, so only case or normalisation collisions on a case-insensitive root apply.

## Escaped-name format (decision 9)
A name a client cannot represent is saved under a `%XX`-escaped name and reported. Each offending scalar is replaced by `%` plus the uppercase hex of its UTF-8 bytes (`a:b` becomes `a%3Ab`). The `%XX` format is decided (decision 9). Two details around it are a **proposed default, to be confirmed in the Step 4 sufficiency question**: a reserved Windows device name gets `%5F` appended (`CON.txt` becomes `CON%5F.txt`), and on a name collision in the folder ` (2)`, ` (3)` is appended. Not reversible by design: the original name stays on the server and in `entry.escaped_from`; the escaped name is local only and never uploaded back as a rename. Tests assert: the file exists under the escaped name, 0 writes outside the root, 0 overwrites, one "name escaped" report per file.

## Hostile path handling stays out of this crate
Traversal, symlink, junction and reparse-point defence needs file handles, so it lives in 14_desktop_daemon `SafeFs`. This crate only guarantees that `split_path` never yields `.`, `..` or a component with a separator.

## Tests
FT-N-41 (server accepts and rejects exactly the list), NFT-SEC-22 (names in the API), FT-N-19 and NFT-SEC-31 (40-case corpus per OS), FT-N-04/05, FT-P-15 (case-only rename keeps the file ID), unit tests for fold stability across `NAME_KEY_VERSION`, `conflict_copy_name` (255-byte truncation with a multi-byte stem, extension kept, label sanitising, numeric suffix on collision).

## Risks
- A Unicode table update changes keys: handled by `name_key_version` and a batch re-key in a later release (data model).
- The escaped-name format is user-visible and test-visible; a literal `%41` in a real name stays distinct from an escaped `A` only through the collision rule, because the escaped name is never decoded.
