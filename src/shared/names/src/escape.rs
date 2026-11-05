use crate::consts::{RESERVED_DEVICE_MARKER, collision_suffix};
use crate::key::{NameKey, nfc};
use crate::lookup::NameLookup;
use crate::name::{MAX_NAME_BYTES, ValidName};
use crate::portable::{
    FsProfile, PLATFORM_UNIT_LIMIT, Portable, Reason, is_windows_forbidden, platform_units,
    portable_check,
};

/// The local-only name a client saves a file under. Never uploaded back as a rename, never decoded.
/// Always a legal name (at most 255 bytes) that the profile can hold and that is not taken in the
/// folder it was computed for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscapedName {
    name: ValidName,
    reasons: Vec<Reason>,
}

impl EscapedName {
    pub fn name(&self) -> &ValidName {
        &self.name
    }

    pub fn as_str(&self) -> &str {
        self.name.as_str()
    }

    /// Why the name changed; empty when it is returned unchanged.
    pub fn reasons(&self) -> &[Reason] {
        &self.reasons
    }

    pub fn was_escaped(&self) -> bool {
        !self.reasons.is_empty()
    }
}

/// Escapes `name` for `profile`. Each offending scalar (Windows: `< > : " | ? * \` and a trailing run of
/// `.`/space) becomes `%` plus the uppercase hex of each of its UTF-8 bytes (`a:b` becomes `a%3Ab`); a
/// reserved device name gets [`RESERVED_DEVICE_MARKER`] after its stem (`CON.txt` becomes `CON%5F.txt`);
/// a name over the platform limit is cut on a scalar boundary; when the result is taken in the folder
/// (`existing`), ` (2)`, ` (3)` is appended until it is free. Deterministic for one folder state, and a
/// name that needs nothing is returned unchanged.
pub fn escape_name(name: &ValidName, profile: FsProfile, existing: &dyn NameLookup) -> EscapedName {
    let mut reasons = match portable_check(name, profile) {
        Portable::Ok => Vec::new(),
        Portable::NeedsEscape(reasons) => reasons,
    };
    let chars: Vec<char> = name.as_str().chars().collect();
    let reserved = reasons.contains(&Reason::ReservedDeviceName);
    let mut number = 1u64;
    loop {
        let suffix = if number == 1 {
            String::new()
        } else {
            collision_suffix(number)
        };
        let candidate = format!(
            "{}{suffix}",
            escape_prefix(&chars, profile, reserved, suffix.len())
        );
        if !existing.is_taken(&NameKey::from_name_str(&candidate)) {
            return EscapedName {
                name: ValidName::new_unchecked(candidate),
                reasons,
            };
        }
        if number == 1 {
            reasons.push(if nfc(name.as_str()) == name.as_str() {
                Reason::CaseCollision
            } else {
                Reason::NfcNfdCollision
            });
        }
        number = number.saturating_add(1);
    }
}

/// The escaped form of the longest prefix of `chars` that leaves `suffix_len` ASCII bytes of room.
fn escape_prefix(chars: &[char], profile: FsProfile, reserved: bool, suffix_len: usize) -> String {
    let fits = |s: &str| {
        s.len() + suffix_len <= MAX_NAME_BYTES
            && platform_units(s, profile) + suffix_len <= PLATFORM_UNIT_LIMIT
    };
    let shortest = |kept: usize| escape_kept(&chars[..kept], chars, profile, reserved);
    for kept in (2..=chars.len()).rev() {
        let escaped = shortest(kept);
        if fits(&escaped) {
            return escaped;
        }
    }
    shortest(chars.len().min(1))
}

fn escape_kept(kept: &[char], all: &[char], profile: FsProfile, reserved: bool) -> String {
    let windows = profile == FsProfile::WindowsNtfs;
    let first_dot = all.iter().position(|&c| c == '.').unwrap_or(all.len());
    let marker_at = first_dot.min(kept.len());
    let trailing_start = if windows {
        kept.iter()
            .rposition(|&c| c != '.' && c != ' ')
            .map_or(0, |i| i + 1)
    } else {
        kept.len()
    };
    let mut out = String::new();
    for (i, &c) in kept.iter().enumerate() {
        if reserved && i == marker_at {
            out.push_str(RESERVED_DEVICE_MARKER);
        }
        if windows && (c == ':' || is_windows_forbidden(c) || i >= trailing_start) {
            push_percent_escaped(&mut out, c);
        } else {
            out.push(c);
        }
    }
    if reserved && marker_at == kept.len() {
        out.push_str(RESERVED_DEVICE_MARKER);
    }
    out
}

fn push_percent_escaped(out: &mut String, c: char) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [0u8; 4];
    for &byte in c.encode_utf8(&mut buf).as_bytes() {
        out.push('%');
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0F)]));
    }
}
