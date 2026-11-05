use sc_clock::Timestamp;

const MICROS_PER_SECOND: i64 = 1_000_000;
const SECONDS_PER_DAY: i64 = 86_400;

/// `YYYY-MM-DD HH-MM` in UTC, floored to the minute (proleptic Gregorian; years outside 0..=9999 print
/// with a sign or more digits, only reachable with extreme timestamps).
pub(crate) fn format_minute(at: Timestamp) -> String {
    let seconds = at.as_unix_micros().div_euclid(MICROS_PER_SECOND);
    let days = seconds.div_euclid(SECONDS_PER_DAY);
    let second_of_day = seconds.rem_euclid(SECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3600;
    let minute = second_of_day % 3600 / 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}-{minute:02}")
}

/// Days since 1970-01-01 to (year, month, day); Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_instants() {
        let at = |micros| format_minute(Timestamp::from_unix_micros(micros));
        assert_eq!(at(0), "1970-01-01 00-00");
        assert_eq!(at(-1), "1969-12-31 23-59");
        assert_eq!(at(951_782_400_000_000), "2000-02-29 00-00");
        assert_eq!(at(1_793_889_059_999_999), "2026-11-05 14-30");
        assert_eq!(at(4_107_542_400_000_000 - 60_000_000), "2100-02-28 23-59");
    }
}
