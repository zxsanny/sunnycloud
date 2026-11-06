use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

/// `YYYY-MM-DDThh:mm:ss` in the user's local time zone, the format of `DeletionDate` in `.trashinfo`.
pub(super) fn local_timestamp(at: SystemTime) -> io::Result<String> {
    let secs = at
        .duration_since(UNIX_EPOCH)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        .as_secs();
    let time =
        libc::time_t::try_from(secs).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    // SAFETY: an all-zero `tm` is a valid value (integers, plus a null `tm_zone` pointer that
    // `localtime_r` overwrites).
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers refer to live, properly aligned locals for the whole call, and
    // `localtime_r` is the re-entrant variant that writes only through `tm`.
    let converted = unsafe { libc::localtime_r(&time, &mut tm) };
    if converted.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    ))
}
