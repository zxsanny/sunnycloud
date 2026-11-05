use sc_clock::Timestamp;

use crate::consts::collision_suffix;
use crate::key::NameKey;
use crate::lookup::NameLookup;
use crate::name::{MAX_NAME_BYTES, ValidName, is_bidi_control};
use crate::utc::format_minute;

/// An extension (text from the last dot, dot included) longer than this is not treated as one: the whole
/// name is the stem. Keeps the fixed part of the copy name bounded so every name has room for a stem.
pub const MAX_EXTENSION_BYTES: usize = 64;

const HEAD: &str = " (conflict copy ";
const TAIL: &str = ")";

/// Builds `stem (conflict copy YYYY-MM-DD HH-MM device).ext` (UTC); the one place conflict-copy names are
/// made. The label has `/`, NUL, control and bidi characters replaced by `_`; the extension (from the last
/// dot, when it is not the first character) is kept; the result is at most 255 bytes, the label being cut
/// first and then the stem, on character boundaries; an empty label drops the label part. When the result
/// is taken in the folder, ` (2)`, ` (3)` goes before the extension. Always passes `validate_name`. The
/// search ends because folders are finite; a lookup that reports every name taken never returns.
pub fn conflict_copy_name(
    contested: &ValidName,
    at: Timestamp,
    device_label: &str,
    taken: &dyn NameLookup,
) -> ValidName {
    let (stem, extension) = split_extension(contested.as_str());
    let label = sanitise_label(device_label);
    let when = format_minute(at);
    let mut number = 1u64;
    loop {
        let suffix = if number == 1 {
            String::new()
        } else {
            collision_suffix(number)
        };
        let candidate = assemble(stem, extension, &when, &label, &suffix);
        if !taken.is_taken(&NameKey::from_name_str(&candidate)) {
            return ValidName::new_unchecked(candidate);
        }
        number = number.saturating_add(1);
    }
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 && name.len() - dot <= MAX_EXTENSION_BYTES => name.split_at(dot),
        _ => (name, ""),
    }
}

fn sanitise_label(label: &str) -> String {
    label
        .chars()
        .map(|c| {
            if c == '/' || c == '\0' || c.is_control() || is_bidi_control(c) {
                '_'
            } else {
                c
            }
        })
        .collect()
}

fn assemble(stem: &str, extension: &str, when: &str, label: &str, suffix: &str) -> String {
    let fixed = HEAD.len() + when.len() + TAIL.len() + suffix.len() + extension.len();
    let room = MAX_NAME_BYTES.saturating_sub(fixed);
    let stem = truncate_on_boundary(stem, room);
    // " " + label needs at least two bytes of room after the stem.
    let label_room = room.saturating_sub(stem.len()).saturating_sub(1);
    let label = truncate_on_boundary(label, label_room);
    let label_part = if label.is_empty() {
        String::new()
    } else {
        format!(" {label}")
    };
    format!("{stem}{HEAD}{when}{label_part}{TAIL}{suffix}{extension}")
}

fn truncate_on_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let cut = (0..=max_bytes)
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    &s[..cut]
}
