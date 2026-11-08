//! AZ-3748 AC-4 over the whole crate: every event of the logging table of the description (§14) is written at its
//! level, every other log line is a listed supporting line, and no line carries anything but the typed `err` and
//! `duration_ms` fields (a blob id and an account id travel inside `err`, whose faults hold ids and a failure class
//! only). Nothing else writes to a log: no `println!`, no `eprintln!`, no direct `tracing` call.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const LEVELS: [(&str, &str); 3] = [
    ("log_error!(", "error"),
    ("log_warn!(", "warn"),
    ("log_info!(", "info"),
];

/// The events of the table: (event, level, message).
const TABLE: [(&str, &str, &str); 19] = [
    ("corrupt chunk on read", "error", "blob corrupt on read"),
    ("key unwrap failure", "error", "data key unwrap failed"),
    ("I/O error on write", "error", "blob write failed"),
    ("failed group sync", "error", "blob group sync failed"),
    ("aborted sweep (GC)", "error", "gc sweep aborted"),
    ("aborted sweep (orphans)", "error", "orphan sweep aborted"),
    ("aborted cycle (scrub)", "error", "scrub cycle failed"),
    (
        "free-space floor hit",
        "warn",
        "blob volume at its free-space floor",
    ),
    ("orphan in report mode", "warn", "orphan blob file"),
    (
        "GC unlink of a missing file",
        "warn",
        "gc blob file already missing",
    ),
    (
        "scrub paused too long",
        "warn",
        "scrub paused until the cycle budget ended",
    ),
    ("cycle start (GC)", "info", "gc cycle started"),
    ("cycle summary (GC)", "info", "gc cycle finished"),
    ("cycle start (orphans)", "info", "orphan cycle started"),
    ("cycle summary (orphans)", "info", "orphan cycle finished"),
    ("cycle start (scrub)", "info", "scrub cycle started"),
    ("cycle summary (scrub)", "info", "scrub cycle finished"),
    ("key rotation batch", "info", "rewrap batch finished"),
    (
        "startup key check passed",
        "info",
        "startup key check passed",
    ),
];

/// Lines the table does not name: other outcomes of the same operations, each with ids and a failure class only.
const SUPPORTING: [(&str, &str); 19] = [
    ("error", "blob finding not recorded"),
    ("error", "blob health probe failed"),
    ("error", "blob volume check did not finish"),
    ("error", "blob volume not inspected"),
    ("error", "gc batch not settled"),
    ("error", "gc batch stopped"),
    ("error", "gc blob file not parked"),
    ("error", "gc blob not removed"),
    ("error", "gc parked file not deleted"),
    ("error", "gc parked file not moved back"),
    ("error", "key check sample failed"),
    ("error", "master key check failed"),
    ("error", "missing blob scan failed"),
    ("error", "orphan blob file not deleted"),
    ("error", "partial blob file not removed"),
    ("error", "data key unwrap refused: master key id not held"),
    ("error", "unwrapped data key has the wrong length"),
    ("warn", "blob folders hold files that are not blob ids"),
    ("info", "rewrap finished, no row left under the old key"),
];

const FIELDS: [&str; 2] = ["err", "duration_ms"];

struct Call {
    file: PathBuf,
    level: &'static str,
    message: String,
    fields: Vec<String>,
}

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("source directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

/// The text between the parentheses of a call that starts at `open`, string literals skipped.
fn arguments(text: &str, open: usize) -> &str {
    let (mut depth, mut in_string, mut escaped) = (0_usize, false, false);
    for (offset, c) in text[open..].char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '(' if !in_string => depth += 1,
            ')' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return &text[open + 1..open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced log call in {text:.80}");
}

fn split_top_level(args: &str) -> Vec<&str> {
    let (mut depth, mut in_string, mut escaped, mut start) = (0_i32, false, false, 0);
    let mut parts = Vec::new();
    for (at, c) in args.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '(' | '[' | '{' if !in_string => depth += 1,
            ')' | ']' | '}' if !in_string => depth -= 1,
            ',' if !in_string && depth == 0 => {
                parts.push(&args[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&args[start..]);
    parts
}

fn calls() -> Vec<Call> {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut found = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).expect("source file");
        for (marker, level) in LEVELS {
            for (at, _) in text.match_indices(marker) {
                let line_start = text[..at].rfind('\n').map_or(0, |newline| newline + 1);
                if text[line_start..at].trim_start().starts_with("//") {
                    continue;
                }
                let open = at + marker.len() - 1;
                let parts = split_top_level(arguments(&text, open));
                let message = parts[0].trim();
                assert!(
                    message.starts_with('"') && message.ends_with('"'),
                    "{}: the message of a log call is a literal: {message}",
                    file.display()
                );
                let fields = parts[1..]
                    .iter()
                    .map(|part| part.trim())
                    .filter(|part| !part.is_empty())
                    .map(|part| part.split('=').next().unwrap_or_default().trim().to_owned())
                    .collect();
                found.push(Call {
                    file: file.clone(),
                    level,
                    message: message.trim_matches('"').to_owned(),
                    fields,
                });
            }
        }
    }
    found
}

#[test]
fn every_event_of_the_logging_table_is_written_at_its_level() {
    // Arrange
    let written: BTreeSet<(&str, String)> = calls()
        .into_iter()
        .map(|call| (call.level, call.message))
        .collect();

    // Act
    let missing: Vec<_> = TABLE
        .iter()
        .filter(|(_, level, message)| !written.contains(&(*level, (*message).to_owned())))
        .map(|(event, level, message)| format!("{event}: {level} {message}"))
        .collect();

    // Assert
    assert!(missing.is_empty(), "events without a line: {missing:?}");
}

#[test]
fn no_other_line_is_written_and_every_line_carries_only_the_typed_fields() {
    // Arrange
    let listed: BTreeSet<(&str, &str)> = TABLE
        .iter()
        .map(|(_, level, message)| (*level, *message))
        .chain(SUPPORTING)
        .collect();

    // Act
    let unlisted: Vec<_> = calls()
        .iter()
        .filter(|call| !listed.contains(&(call.level, call.message.as_str())))
        .map(|call| format!("{} {}: {}", call.file.display(), call.level, call.message))
        .collect();
    let stray_fields: Vec<_> = calls()
        .iter()
        .flat_map(|call| call.fields.iter().map(move |field| (call, field)))
        .filter(|(_, field)| !FIELDS.contains(&field.as_str()))
        .map(|(call, field)| format!("{}: {field}", call.message))
        .collect();

    // Assert
    assert!(unlisted.is_empty(), "lines outside the table: {unlisted:?}");
    assert!(stray_fields.is_empty(), "fields: {stray_fields:?}");
}

#[test]
fn nothing_writes_to_a_log_but_the_typed_macros() {
    // Arrange
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );

    // Act
    let offenders: Vec<_> = files
        .iter()
        .filter(|file| {
            let text = fs::read_to_string(file).expect("source file");
            ["println!(", "eprintln!(", "dbg!(", "tracing::"]
                .iter()
                .any(|call| {
                    text.lines()
                        .any(|line| !line.trim_start().starts_with("//") && line.contains(call))
                })
        })
        .collect();

    // Assert
    assert!(offenders.is_empty(), "{offenders:?}");
}
