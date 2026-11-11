//! The wire form of a [`Timestamp`]: `2026-11-06T10:11:12.123456Z`, UTC with microseconds, always `Z`. A client
//! may send up to six fractional digits, or none.

use sc_clock::Timestamp;

pub(crate) fn rfc3339(at: Timestamp) -> String {
    let micros = at.as_unix_micros();
    let secs = micros.div_euclid(1_000_000);
    let sub = micros.rem_euclid(1_000_000);
    let days = secs.div_euclid(86_400);
    let in_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{sub:06}Z",
        in_day / 3600,
        in_day % 3600 / 60,
        in_day % 60
    )
}

/// The instant of `YYYY-MM-DDTHH:MM:SS[.f]Z` (one to six fractional digits, UTC only); `None` for anything else.
pub(crate) fn parse_rfc3339(text: &str) -> Option<Timestamp> {
    let (date, time) = text.strip_suffix('Z')?.split_once('T')?;
    let (clock, fraction) = match time.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (time, None),
    };
    let [year, month, day] = fixed_numbers::<3>(date, '-', [4, 2, 2])?;
    let [hour, minute, second] = fixed_numbers::<3>(clock, ':', [2, 2, 2])?;
    let micros = match fraction {
        None => 0,
        Some(digits) if (1..=6).contains(&digits.len()) => {
            let padded = format!("{digits:0<6}");
            number(&padded)?
        }
        Some(_) => return None,
    };
    let days = days_from_civil(year, month, day);
    if civil_from_days(days) != (year, month, day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second;
    Some(Timestamp::from_unix_micros(
        secs.checked_mul(1_000_000)?.checked_add(micros)?,
    ))
}

/// `N` numbers of the given digit counts joined by `separator`.
fn fixed_numbers<const N: usize>(
    text: &str,
    separator: char,
    widths: [usize; N],
) -> Option<[i64; N]> {
    let parts: Vec<&str> = text.split(separator).collect();
    if parts.len() != N {
        return None;
    }
    let mut numbers = [0; N];
    for ((slot, part), width) in numbers.iter_mut().zip(parts).zip(widths) {
        if part.len() != width {
            return None;
        }
        *slot = number(part)?;
    }
    Some(numbers)
}

fn number(digits: &str) -> Option<i64> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Day count since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let shifted_month = (month + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Proleptic Gregorian date of a day count since 1970-01-01 (Howard Hinnant's `civil_from_days`).
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

    fn at(micros: i64) -> String {
        rfc3339(Timestamp::from_unix_micros(micros))
    }

    #[test]
    fn epoch_leap_day_and_before_epoch_are_written_in_utc_with_microseconds() {
        // Arrange
        let instants = [
            0,
            951_782_400_000_000,
            1_793_956_272_123_456,
            -1,
            -62_135_596_800_000_000,
        ];

        // Act
        let written: Vec<String> = instants.into_iter().map(at).collect();

        // Assert
        assert_eq!(
            written,
            [
                "1970-01-01T00:00:00.000000Z",
                "2000-02-29T00:00:00.000000Z",
                "2026-11-06T09:11:12.123456Z",
                "1969-12-31T23:59:59.999999Z",
                "0001-01-01T00:00:00.000000Z",
            ]
        );
    }

    #[test]
    fn a_written_instant_is_read_back_and_a_client_may_send_fewer_digits() {
        // Arrange
        let micros = [0, 951_782_400_000_000, 1_793_956_272_123_456, -1];

        // Act
        let read_back: Vec<Option<Timestamp>> = micros
            .into_iter()
            .map(|at| parse_rfc3339(&rfc3339(Timestamp::from_unix_micros(at))))
            .collect();
        let short = [
            parse_rfc3339("2026-11-06T09:11:12Z"),
            parse_rfc3339("2026-11-06T09:11:12.5Z"),
            parse_rfc3339("2026-11-06T09:11:12.123Z"),
        ];

        // Assert
        let expected: Vec<Option<Timestamp>> = micros
            .into_iter()
            .map(|at| Some(Timestamp::from_unix_micros(at)))
            .collect();
        assert_eq!(read_back, expected);
        assert_eq!(
            short,
            [
                Some(Timestamp::from_unix_micros(1_793_956_272_000_000)),
                Some(Timestamp::from_unix_micros(1_793_956_272_500_000)),
                Some(Timestamp::from_unix_micros(1_793_956_272_123_000)),
            ]
        );
    }

    #[test]
    fn text_that_is_not_a_utc_instant_is_refused() {
        // Arrange
        let cases = [
            "",
            "2026-11-06",
            "2026-11-06T09:11:12",
            "2026-11-06T09:11:12+02:00",
            "2026-11-06T09:11:12.1234567Z",
            "2026-11-06T09:11:12.Z",
            "2026-02-30T09:11:12Z",
            "2026-13-06T09:11:12Z",
            "2026-11-06T24:00:00Z",
            "2026-11-06T09:60:00Z",
            "2026-11-06T09:11:60Z",
            "26-11-06T09:11:12Z",
            "2026-11-06 09:11:12Z",
            "+026-11-06T09:11:12Z",
        ];

        // Act
        let parsed: Vec<bool> = cases
            .iter()
            .map(|case| parse_rfc3339(case).is_some())
            .collect();

        // Assert
        assert_eq!(parsed, [false; 14]);
    }
}
