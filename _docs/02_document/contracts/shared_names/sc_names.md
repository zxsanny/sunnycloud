# Contract: sc_names — legal names, name keys, conflict copies, client portability

**Component**: shared/names (crate `sc_names`; pure, builds native and `wasm32-unknown-unknown`)
**Producer task**: E-05/c01 — `_docs/02_tasks/todo/B01-11_sc_names.md` (renamed to its AZ key at Step 4)
**Consumer tasks**: other batches: 02 (E-13), 03 (E-14), 12 (E-22), `sync-core-proto` and `sync-core` (E-23), 14 (E-24), `mobile_backup` (E-27), 16 tests (E-26)
**Version**: 1.0.0
**Status**: draft
**Last Updated**: 2026-11-04

## Purpose

One definition of a legal name, of the case-insensitive key, of the server-assigned conflict-copy name and of what a client does with a name its file system cannot hold. The server accepts everything except a short list; clients carry the portability burden from the same shared corpus.

Sources: `common-helpers/06_helper_names.md`, `epics/E-05_cross-cutting-name-rules.md`, `component-decisions.md` decisions 9 and 15, `canonical-names.md` (server assigns the conflict-copy name).

## Shape

| Name | Signature | Errors | Blocking? |
|---|---|---|---|
| `validate_name` | `fn validate_name(s: &str) -> Result<ValidName, NameError>` | `NameError::{Empty, TooLong(bytes), Dot, DotDot, ContainsSlash, ContainsNul, ControlChar, BidiControl}` | sync, pure |
| `name_key` | `fn name_key(name: &ValidName) -> NameKey` (NFC, then Unicode full case folding) | none | sync, pure |
| `NAME_KEY_VERSION` | constant: the pinned Unicode version id stored in `nodes.name_key_version` | | |
| `portable_check` | `fn portable_check(name: &ValidName, profile: FsProfile) -> Portable` | none | sync, pure |
| `escape_name` | `fn escape_name(name: &ValidName, profile: FsProfile, existing: &dyn NameLookup) -> EscapedName` | none | sync, pure |
| `split_path` | `fn split_path(path: &str) -> Result<Vec<ValidName>, NameError>` (relative, `/`-separated) | `NameError` for any component | sync, pure |
| `conflict_copy_name` | `fn conflict_copy_name(contested: &ValidName, at: Timestamp, device_label: &str, taken: &dyn NameLookup) -> ValidName` | none | sync, pure |
| `NameLookup` | trait: `fn is_taken(&self, key: &NameKey) -> bool` (is this name taken in the folder; database-backed or in-memory) | | |

| Type | Shape |
|---|---|
| `ValidName` | a name that passed `validate_name` (at most 255 bytes UTF-8) |
| `NameKey` | the folded key (`COLLATE "C"` makes uniqueness byte-exact) |
| `FsProfile` | `WindowsNtfs`, `PosixCaseSensitive`, `CaseInsensitivePreserving` |
| `Portable` | `Ok` or `NeedsEscape(Vec<Reason>)` |
| `Reason` | reserved Windows device name, trailing dot or space, one of `< > : " \| ? * \`, alternate-data-stream syntax, over the platform length limit, lone surrogate, case collision with a sibling, NFC/NFD collision |
| `EscapedName` | local-only name; never uploaded back as a rename, never decoded |

Format rules:
- Server rejects only: whole name `.` or `..`; `/`; NUL; Unicode Cc; bidi controls U+061C, U+200E, U+200F, U+202A-U+202E, U+2066-U+2069; empty; over 255 bytes.
- Conflict copy: `stem (conflict copy YYYY-MM-DD HH-MM device).ext`, UTC; `device` sanitised (`/`, NUL, control and bidi characters become `_`); extension kept; truncation on a character boundary within 255 bytes, label cut first; on collision ` (2)`, ` (3)` before the extension.
- Escape: each offending scalar becomes `%` plus the uppercase hex of its UTF-8 bytes (`a:b` -> `a%3Ab`); proposed defaults still open with the owner, kept as named constants: reserved device name gets `%5F` (`CON.txt` -> `CON%5F.txt`), collision suffix ` (2)`.
- Shared corpus `names_corpus.json`: 40 hostile cases per OS, consumed by server, daemon and web tests.

## Invariants

- No I/O, no global state, no dependency on `sc_db`; depends on `sc_clock` for `Timestamp` only.
- `validate_name(conflict_copy_name(x, ...))` always succeeds; `name_key` is idempotent and stable for one `NAME_KEY_VERSION`.
- `split_path` never yields `.`, `..` or a component containing a separator.
- `escape_name` is deterministic and collision-free within the folder state given.
- `name_key` is computed in Rust only, never in SQL.

## Non-Goals

- Not covered: symlink, junction and reparse-point defence (14 `SafeFs`); file-system profile detection (14); batch re-key after a Unicode change (later release).

## Versioning Rules

- Changing the accepted set, the fold or `NAME_KEY_VERSION` is breaking (stored keys change). Changing the two open defaults is a minor change once the owner decides, because escaped names are local only.

## Test Cases

| Case | Input | Expected | Notes |
|---|---|---|---|
| accept-reject | every rejected class and `CON`, `a.`, `a `, `a\b`, `a:b`, `-x`, emoji | rejected exactly the list; the rest accepted | AC-37, FT-N-41 |
| case-key | `Readme.md`, `README.MD` | same `NameKey`; stored name keeps its case | AC-42 |
| copy-fits | multi-byte stem near 255 bytes, long label | <= 255 bytes, extension kept, valid | AC-8 |
| copy-collision | copy name already taken | ` (2)` before the extension | |
| escape-det | `a:b` and `CON.txt` on `WindowsNtfs`, twice | `a%3Ab`, `CON%5F.txt`, same on repeat | |
| traversal | `split_path("a/../b")`, `"./a"` | `NameError` | |

## Open points (flagged in the producer task)

1. The Windows reserved-name marker (`%5F`) and the ` (2)` escape suffix are owner items (canonical-names "Open owner items").
2. The Unicode normalisation and case-folding library is not chosen; the task picks one whose Unicode version can be pinned and reported.
3. NFC versus NFD as the stored form is an owner item routed through E-13's spike (G-1).
4. Helper 06 defines `NameLookup` only as "is this name taken in the folder"; the method name `is_taken(&NameKey)` is introduced here.

## Change Log

| Version | Date | Change | Author |
|---|---|---|---|
| 1.0.0 | 2026-11-04 | Initial contract | decompose Step 2 (B01) |
