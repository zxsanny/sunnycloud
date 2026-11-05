//! Open owner items of helper 06 kept as named constants in one place (changing them is a minor
//! change: escaped names are local only). Tests assert the current choice.

/// Appended to the stem of a reserved Windows device name: `CON.txt` becomes `CON%5F.txt`.
pub const RESERVED_DEVICE_MARKER: &str = "%5F";

/// A collision suffix is `OPEN` + number + `CLOSE`: ` (2)`, ` (3)`.
pub const COLLISION_SUFFIX_OPEN: &str = " (";
/// See [`COLLISION_SUFFIX_OPEN`].
pub const COLLISION_SUFFIX_CLOSE: &str = ")";

pub(crate) fn collision_suffix(number: u64) -> String {
    format!("{COLLISION_SUFFIX_OPEN}{number}{COLLISION_SUFFIX_CLOSE}")
}
